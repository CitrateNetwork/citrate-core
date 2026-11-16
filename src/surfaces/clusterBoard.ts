// 0.5.4 FL design, board 04 (Groups › Cluster): the pure view models behind the cluster board.
//
// Data sources (Rule 7), and only these:
//   - this machine's hardware: `fleet_probe` (its TierReport facts). Probed locally, no network.
//   - your paired machines: `fleet_roster` (label + tier class from the pairing).
//   - the group's machines: `cluster_peers` (online, direct / relayed) and `cluster_devices` (labels).
//   - round requirements: none today. Group rounds are not live, so there is nothing to check this
//     machine against. `RoundRequirements` is the interface a live round will fill (FL lane 1).
// Members' machines do not share hardware reports today, so they are "not reported": shown as "—"
// with the reason and never counted as zero (unknown is not zero).
import type { HardwareFacts } from "../bridge/domains";
import type { FleetDevice, JoinResult } from "../bridge/tauri/fleet";

export const GIB = 2 ** 30;

/** Whole gigabytes, as the design writes them ("24 GB"). */
export function gb(bytes: number): string {
  return `${Math.round(bytes / GIB)} GB`;
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

export const NO_ROUNDS_REASON = "Group rounds not live yet, so there are no round requirements to check this machine against.";

export function eligibilityModel(probe: ProbeReading, req: RoundRequirements | null): Eligibility {
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
  if (!req) return { kind: "unknown", word: "Not known", lines: [NO_ROUNDS_REASON, `This machine reported: ${hardwareLine(f)}.`], reprobe: false };

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
  /** Machines in the cluster that have not reported. */
  notReported: number;
  /** Your paired machines, with the tier class each reported when it paired. */
  paired: { id: string; label: string; tier: string }[];
}

export function capacityModel(probe: ProbeReading, peerCount: number, paired: FleetDevice[]): Capacity {
  const facts = probe.kind === "ok" ? probe.facts : null;
  const acc = facts ? acceleratorBytes(facts) : null;
  return {
    reported: facts ? 1 : 0,
    accelerator: acc != null ? gb(acc) : "—",
    ram: facts?.totalRamBytes != null ? gb(facts.totalRamBytes) : "—",
    notReported: peerCount + (facts ? 0 : 1),
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
