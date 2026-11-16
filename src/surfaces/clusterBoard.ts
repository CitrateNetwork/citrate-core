// 0.5.4 FL design, board 04 (Groups › Cluster): the pure view models behind the cluster board.
//
// Data sources (Rule 7), and only these:
//   - this machine's hardware: `fleet_probe` (its TierReport facts). Probed locally, no network.
//   - your paired machines: `fleet_roster` (label + tier class from the pairing).
//   - the group's machines: `cluster_peers` (online, direct / relayed) and `cluster_devices` (labels).
//   - whether rounds are live: the FL network state (src/fl/networkState.ts, `useFlNetworkState`),
//     read from the address book and the chain. Phase "today": Group rounds not live yet, so there
//     is nothing to check this machine against. `RoundRequirements` is what a live round fills.
// Members' machines do not share hardware reports today, so they are "not reported": shown as "—"
// with the reason and never counted as zero (unknown is not zero).
import type { HardwareFacts } from "../bridge/domains";
import type { FleetDevice, JoinResult } from "../bridge/tauri/fleet";
import { ROUNDS_NOT_LIVE, type FlPhase } from "../fl/networkState";

export const GIB = 2 ** 30;

/** Whole gibibytes ("24 GiB"): the probe reports bytes and this divides by 2^30. */
export function gb(bytes: number): string {
  return `${Math.round(bytes / GIB)} GiB`;
}

/** The probe of this machine: still running, read, or failed with its reason. */
export type ProbeReading = { kind: "loading" } | { kind: "ok"; facts: HardwareFacts } | { kind: "failed"; reason: string };

/** Memory a round could train on: dedicated GPU memory, else unified memory on Apple silicon. */
export function acceleratorBytes(f: HardwareFacts): number | null {
  if (f.gpuVramBytes != null && f.gpuVramBytes > 0) return f.gpuVramBytes;
  if (f.unifiedMemory === true && f.totalRamBytes != null) return f.totalRamBytes;
  return null;
}

/** One-line hardware receipt for this machine, from its probe. */
export function hardwareLine(f: HardwareFacts): string {
  const parts: string[] = [];
  if (f.gpuVramBytes != null && f.gpuVramBytes > 0) parts.push(`GPU, ${gb(f.gpuVramBytes)} memory`);
  else if (f.unifiedMemory === true) parts.push(f.totalRamBytes != null ? `Apple silicon, ${gb(f.totalRamBytes)} unified memory` : "Apple silicon, unified memory size not read");
  else parts.push("no GPU memory reported");
  if (f.totalRamBytes != null && f.unifiedMemory !== true) parts.push(`${gb(f.totalRamBytes)} RAM`);
  if (f.diskFreeBytes != null) parts.push(`${gb(f.diskFreeBytes)} disk free`);
  return parts.join(", ");
}

/** What a live round asks of a machine (from its requirements document). All fields optional. */
export interface RoundRequirements {
  /** How the round is named to the member, e.g. "round 15". */
  round: string;
  minAcceleratorBytes?: number;
  minRamBytes?: number;
  minDiskFreeBytes?: number;
}

export interface Eligibility {
  kind: "eligible" | "not-eligible" | "unknown" | "checking";
  word: string;
  /** One line per requirement: met, the "why not" for a miss, or why it can't be checked. */
  lines: string[];
  /** Offer "Run the probe again" (the probe failed). */
  reprobe: boolean;
}

export const NO_ROUNDS_REASON = `${ROUNDS_NOT_LIVE}, so there are no round requirements to check this machine against.`;

/** What the FL network state says about rounds (its phase, and why when it is unknown). */
export interface RoundsContext {
  phase: FlPhase;
  reason?: string;
}

/** Why there is nothing to check against yet, from the network's phase (requirements absent). */
export function noRequirementsReason(rounds: RoundsContext): string {
  switch (rounds.phase) {
    case "today":
      return NO_ROUNDS_REASON;
    case "reading":
      return "Reading whether group rounds are live on this network.";
    case "unknown":
      return `Whether group rounds are live is unknown (${rounds.reason ?? "a contract read failed"}), so this machine can't be checked yet.`;
    case "rounds-live":
      return "No open round's requirements have been read for this machine yet.";
  }
}

export function eligibilityModel(probe: ProbeReading, req: RoundRequirements | null, rounds: RoundsContext = { phase: "today" }): Eligibility {
  if (probe.kind === "loading") return { kind: "checking", word: "Checking", lines: ["Reading this machine's hardware."], reprobe: false };
  if (probe.kind === "failed") {
    return {
      kind: "unknown",
      word: "Not known",
      lines: [`The hardware probe failed: ${probe.reason}`, "Eligibility is not shown as zero; it is unknown until the probe runs."],
      reprobe: true,
    };
  }
  const f = probe.facts;
  if (!req) return { kind: "unknown", word: "Not known", lines: [noRequirementsReason(rounds), `This machine reported: ${hardwareLine(f)}.`], reprobe: false };

  const lines: string[] = [];
  let miss = false;
  let unknown = false;
  const check = (have: number | null, need: number | undefined, what: string) => {
    if (need == null) return;
    if (have == null) {
      unknown = true;
      lines.push(`${what} not reported, so the ${gb(need)} minimum of ${req.round} can't be checked`);
    } else if (have >= need) {
      lines.push(`${gb(have)} ${what} meets the ${gb(need)} minimum of ${req.round}`);
    } else {
      miss = true;
      lines.push(`needs ${gb(need)} ${what} for ${req.round} (this machine has ${gb(have)})`);
    }
  };
  check(acceleratorBytes(f), req.minAcceleratorBytes, "GPU memory");
  check(f.totalRamBytes, req.minRamBytes, "RAM");
  check(f.diskFreeBytes, req.minDiskFreeBytes, "free disk");
  if (lines.length === 0) lines.push(`${req.round} sets no hardware minimum`);
  if (miss) return { kind: "not-eligible", word: "Not eligible right now", lines, reprobe: false };
  if (unknown) return { kind: "unknown", word: "Not known", lines, reprobe: false };
  return { kind: "eligible", word: "Eligible", lines, reprobe: false };
}

/** Combined capacity, counting only machines that reported. */
export interface Capacity {
  /** Machines with a hardware receipt (this machine when its probe read). */
  reported: number;
  /** GPU memory reported, as text ("—" when nothing reported it). */
  accelerator: string;
  /** RAM reported, as text. */
  ram: string;
  /** Machines in the cluster that have not reported; null when the cluster has not been read. */
  notReported: number | null;
  /** Your paired machines, with the tier class each reported when it paired. */
  paired: { id: string; label: string; tier: string }[];
}

/** `peerCount` is null while the cluster read is loading or failed: unknown, never zero. */
export function capacityModel(probe: ProbeReading, peerCount: number | null, paired: FleetDevice[]): Capacity {
  const facts = probe.kind === "ok" ? probe.facts : null;
  const acc = facts ? acceleratorBytes(facts) : null;
  return {
    reported: facts ? 1 : 0,
    accelerator: acc != null ? gb(acc) : "—",
    ram: facts?.totalRamBytes != null ? gb(facts.totalRamBytes) : "—",
    notReported: peerCount === null ? null : peerCount + (facts ? 0 : 1),
    paired: paired.map((d) => ({ id: d.id, label: d.label, tier: d.tier ? `tier ${d.tier}` : "tier not reported" })),
  };
}

/** Plain words for every pairing outcome on the joining machine. */
export function joinOutcome(r: JoinResult): { ok: boolean; text: string } {
  if (r.ok && r.device) {
    const code = r.device.code ? ` Both machines show code ${r.device.code}; check they match.` : "";
    return { ok: true, text: `Paired with ${r.device.label}.${code}` };
  }
  const why = r.message ? ` (${r.message})` : "";
  switch (r.errorKind) {
    case "link":
      return { ok: false, text: `That link is not a valid pairing link, or it has expired. Ask for a new one${why}.` };
    case "refused":
      return { ok: false, text: `The other machine refused the pairing${why}.` };
    case "unreachable":
      return {
        ok: false,
        text: `Couldn't reach the other machine at any address in the link (${r.tried.length} tried). Check both machines are on the same network, or use the connectivity help under Your devices and pairing.`,
      };
    case "protocol":
      return { ok: false, text: `The other machine answered in a way this app does not understand. Update Citrate Core on both machines and try again${why}.` };
    default:
      return { ok: false, text: `The pairing did not finish${why}.` };
  }
}

/** A pasted link is a group seed (dial the group's machines) rather than a pairing link. */
export function isGroupSeed(link: string): boolean {
  return link.trim().startsWith("citrate-cluster://");
}

/** Machines that appeared in the roster since the sheet opened (the other machine joined). */
export function newlyPaired(before: ReadonlySet<string>, now: FleetDevice[]): FleetDevice[] {
  return now.filter((d) => !before.has(d.id));
}

/**
 * The machines that paired by redeeming THIS offer: new since the sheet opened, recorded as issued
 * by this machine, and paired no earlier than the offer was made (and no later than it expired).
 * A machine that paired through another link (the wizard, an older offer) is not this one.
 */
export function joinedThisOffer(before: ReadonlySet<string>, now: FleetDevice[], offer: { issuedAt: number; expiresAt: number }): FleetDevice[] {
  return newlyPaired(before, now).filter((d) => d.via === "issued" && d.pairedAt >= offer.issuedAt && d.pairedAt <= offer.expiresAt + 5);
}

/** What to do when the two six-digit codes differ (both sides say it the same way). */
export const CODE_MISMATCH = "If the codes differ, a different machine used the link. Remove it here, and make a new link.";

/** What the cluster read says about other machines: read, or unknown and why. */
export type PeersRead = { kind: "read" } | { kind: "unknown"; reason: string };

export function peersRead(st: { selectedId: string | null; loading: boolean; error: string | null; status: unknown }): PeersRead {
  if (!st.selectedId) return { kind: "unknown", reason: "no group selected" };
  if (st.loading) return { kind: "unknown", reason: "reading the cluster" };
  if (st.error) return { kind: "unknown", reason: `the cluster read failed: ${st.error}` };
  if (!st.status) return { kind: "unknown", reason: "the cluster daemon has not answered" };
  return { kind: "read" };
}
