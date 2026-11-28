// L7 bridge namespace `bridge.net` (planset v060, lanes/L0_INTEGRATION_SEAMS.md section 3.3):
// the tailnet, Cloudflare, device pairing and mesh state. L7 owns this file and its two impls, src/bridge/tauri/l7.ts and
// src/bridge/sim/l7.ts (.agentile/v60-ownership.map); no other lane edits or imports them.
//
// It starts with no members. Each member L7 adds names its data source (Rule 7) and lands with
// both impls: the tauri one invokes a registered L7 command, the sim one answers honestly or throws
// `Unavailable` (src/bridge/types.ts), never an invented value (Rule 1). Types only in this file.
import type { MeshStatus } from "../domains/cluster";
import type { TailscaleView } from "../tauri/fleet";

export interface NetDomain {}

// ---- L7-WP01: the lane's frozen link vocabulary (lanes/L7_NETWORKING.md section 3.2) ----------
// Mirrors core `fleet::net::link_security`. The names are pinned on both sides by the shared fixture
// src-tauri/src/net/fixtures/net_status_v1.json (Rust: link_security_tests.rs `fixture_json`;
// TS: src/net/netTypes.test.ts). The commands that return these shapes (`net_status`,
// `net_machines`, `net_link_checklist`) land in later L7 work packages; until then nothing returns them.

/** The private link a machine uses to reach the member's other machines. */
export type LinkKind = "tailnet-own" | "tailnet-managed" | "warp" | "lan" | "none";

/**
 * How far a link's protection goes. Only `pq-hybrid-e2e` may ever be shown as post-quantum, and
 * only when core established the hybrid session with the expected suite on that link.
 */
export type LinkSecurity =
  | "pq-hybrid-e2e" // L9 hybrid ML-KEM session verified end to end on this link
  | "classical-e2e" // end-to-end encrypted, classical only (Noise or WireGuard without the PQ layer)
  | "terminated-at-provider" // TLS ends at the provider (Cloudflare edge); the provider can read it
  | "none"; // plaintext (only loopback is ever allowed here)

/** The Tailscale reading core already returns (`fleet_tailscale::TailscaleReport`). */
export type TailscaleReport = TailscaleView["report"];

export interface NetStatus {
  /** The member's choice, or "none". */
  link: LinkKind;
  tailscale: TailscaleReport;
  tailscaleVia: "localapi" | "cli" | "none";
  warp: { present: boolean; ips: string[] };
  /** Addresses the mesh and pairing may bind. */
  privateAddrs: string[];
  pairingPort: number;
  mesh: MeshStatus;
  meshLinkSecurity: LinkSecurity;
  /** Plain words, no claims beyond the fields above. */
  note: string;
}

export interface MachineView {
  /** Fleet id (`fleet.rs` FleetDevice.id). */
  id: string;
  label: string;
  role: string;
  tier: string | null;
  thisMachine: boolean;
  /** From the DeviceLink, if linked. */
  deviceAddress: string | null;
  linked: "linked-under-you" | "paired-not-linked" | "revoked";
  /** MagicDNS name when matched. */
  tailnetHost: string | null;
  /** Tailscale peer online state; null when unknown. */
  tailnetOnline: boolean | null;
  /** Unix seconds, mesh or tailnet. */
  lastSeen: number | null;
  linkSecurity: LinkSecurity;
}

export interface LinkChecklist {
  done: boolean;
  step: "choose-link" | "install-tailscale" | "sign-in" | "pair" | "link" | "done";
  text: string;
}
