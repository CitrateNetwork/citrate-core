// Bridge — fleet wizard (HUP-S8.2 + S8.3), TAURI. Rust `fleet.rs`:
// - fleet_probe: this machine's fleet id, label, tier report and role (local, no network);
// - fleet_discovery_set / fleet_discovery_browse: OPT-IN mDNS (off by default, off at restart);
// - fleet_pair_create / fleet_pair_inspect / fleet_pair_join: short-lived, single-use, signed links;
// - fleet_tailscale: read-only `tailscale status --json` + guidance. Never changes Tailscale.
// Shapes mirror the Rust serde (camelCase).
import { invoke } from "./invoke";
import type { TierId, TierReport } from "../domains";

export type FleetRole = "light" | "worker" | "heavy" | "unknown";

export interface FleetSelf {
  deviceId: string;
  label: string;
  tier: TierId | null;
  role: FleetRole;
}

export interface FleetProbe {
  device: FleetSelf;
  tier: TierReport;
}

/** What happened to the other machine's DeviceLink code in a pairing (core `PairedLink`). */
export type PairedLink = "none" | "added" | "otherMember" | "refused";

export interface FleetDevice {
  id: string;
  label: string;
  tier: TierId | null;
  role: FleetRole;
  addr: string | null;
  pairedAt: number;
  via: "issued" | "joined";
  /** HUP-S8.1: absent when no link code came with the pairing. */
  deviceLink?: PairedLink;
  /** Six digits both machines show for this pairing (absent for older entries). */
  code?: string | null;
}

export interface FleetAdvert {
  id: string;
  label: string;
  tier: TierId | null;
  role: FleetRole;
  port: number;
}

export interface SeenDevice {
  advert: FleetAdvert;
  ip: string;
  ageSecs: number;
}

export interface QrMatrix {
  size: number;
  rows: string[];
}

export interface PairOffer {
  link: string;
  qr: QrMatrix;
  expiresAt: number;
  hints: string[];
  /** For a machine without Citrate Core: the install page and its QR. */
  installUrl?: string;
  installQr?: QrMatrix;
  /** This machine is linked, so its DeviceLink travels with the pairing. */
  carriesDeviceLink?: boolean;
}

/** Whether THIS machine has a DeviceLink (from `device_links`). */
export interface LinkStatus {
  linked: boolean;
  label: string | null;
}

export interface PairClaim {
  v: number;
  nonce: string;
  issuerPub: string;
  issuerLabel: string;
  issuerTier: TierId | null;
  issuedAt: number;
  expiresAt: number;
  hints: string[];
}

export type JoinErrorKind = "link" | "refused" | "unreachable" | "protocol";

export interface JoinResult {
  ok: boolean;
  device: FleetDevice | null;
  errorKind: JoinErrorKind | null;
  message: string | null;
  tried: string[];
}

export type TsState = "notInstalled" | "notRunning" | "needsLogin" | "stopped" | "starting" | "running" | "unknown";

export interface TsPeer {
  hostName: string;
  os: string;
  ips: string[];
  online: boolean;
}

export interface TailscaleView {
  report: {
    state: TsState;
    version: string | null;
    selfHost: string | null;
    selfIps: string[];
    peers: TsPeer[];
  };
  guidance: { id: string; text: string; url: string | null }[];
}

/** Everything the wizard needs from the backend. Injected, so tests can drive the wizard. */
export interface FleetApi {
  probe(): Promise<FleetProbe>;
  setLabel(label: string): Promise<FleetSelf>;
  roster(): Promise<FleetDevice[]>;
  setDiscovery(enabled: boolean): Promise<{ enabled: boolean }>;
  browse(): Promise<SeenDevice[]>;
  createLink(): Promise<PairOffer>;
  inspectLink(link: string): Promise<PairClaim>;
  joinLink(link: string): Promise<JoinResult>;
  tailscale(lanPeers: number, unreachable: boolean): Promise<TailscaleView>;
  /** HUP-S8.1: whether this machine is linked (optional: the wizard works without it). */
  linkStatus?(): Promise<LinkStatus>;
  /** HUP-S8.1: open the wallet ceremony that links this machine (signs nothing by itself). */
  linkThisDevice?(label: string): Promise<void>;
}

/** Link status from the cluster bridge's `myDevices` (the wizard's view of `device_links`). */
export function linkStatusOf(d: { thisDevice: string | null; links: { device: string; label: string }[] }): LinkStatus {
  const mine = d.thisDevice ? d.links.find((l) => l.device === d.thisDevice) : undefined;
  return { linked: !!mine, label: mine?.label ?? null };
}

export const tauriFleet: FleetApi = {
  probe: () => invoke<FleetProbe>("fleet_probe"),
  setLabel: (label) => invoke<FleetSelf>("fleet_set_label", { label }),
  roster: () => invoke<FleetDevice[]>("fleet_roster"),
  setDiscovery: (enabled) => invoke<{ enabled: boolean }>("fleet_discovery_set", { enabled }),
  browse: () => invoke<SeenDevice[]>("fleet_discovery_browse"),
  createLink: () => invoke<PairOffer>("fleet_pair_create"),
  inspectLink: (link) => invoke<PairClaim>("fleet_pair_inspect", { link }),
  joinLink: (link) => invoke<JoinResult>("fleet_pair_join", { link }),
  tailscale: (lanPeers, unreachable) => invoke<TailscaleView>("fleet_tailscale", { lanPeers, unreachable }),
};
