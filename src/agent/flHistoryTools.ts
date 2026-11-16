// =====================================================================
// citrate-core: Hermes's FL history and patronage reads (0.5.4, Hermes x FL WP-H10, §5.6)
//
//   fl_history_read    this device's FL record: round starts, consents, gate decisions, payouts  (read)
//   fl_patronage_read  the member's PatronageLedger units and pending dividend               (read)
//
// Data sources (Rule 7):
//  - `fl_overview` (src-tauri fl_rounds.rs, `fl_rounds.json` and the worker's consent file): the
//    round starts the member approved, which ones were withdrawn, the rounds consented to, the eval
//    gate's decisions, the adapter served now;
//  - payouts: which FL contracts this network has (address book plus eth_getCode, networkState.ts)
//    and the network profile's `payoutsOpen`. On r1007 WorkMarket is not pinned, so there is no
//    payout history to read and the answer says so in the required words;
//  - `training_status` / `training_reward` (src-tauri training.rs): PatronageLedger
//    `roundMergedHash`, `units(member)` and `pendingDividendOf(member)` by eth_call. A failed read
//    is an error with its reason, never 0.
//
// Gate records' base model names come from scorecard files, so this history leaves them out (the
// fenced detail is fl_adapter_gate_read's). The web preview has no core and says so rather than
// reporting the preview's empty values.
// =====================================================================
import type { FlNetworkState } from "../fl/networkState";
import { NOT_AVAILABLE, PAYOUTS_DARK } from "../fl/networkState";
import { saltText } from "../fl/format";
import type { FlOverview, RewardInfo } from "../bridge/domains";

export const FL_HISTORY_TOOL_NAMES = ["fl_history_read", "fl_patronage_read"] as const;
export type FlHistoryToolName = (typeof FL_HISTORY_TOOL_NAMES)[number];

export function isFlHistoryTool(name: string): name is FlHistoryToolName {
  return (FL_HISTORY_TOOL_NAMES as readonly string[]).includes(name);
}

export interface FlHistoryDeps {
  /** True inside the desktop app (core is there); false in the web preview. */
  desktop: boolean;
  overview: () => Promise<FlOverview>;
  networkState: () => Promise<FlNetworkState>;
  reward: (groupId: string) => Promise<RewardInfo>;
}

export const NEEDS_DESKTOP = "FL history and patronage are read by the desktop app's core; this preview has none, so nothing was read.";

const FILTERS = ["all", "rounds", "consent", "gate", "payouts"] as const;
type Filter = (typeof FILTERS)[number];

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const at = (ms: number) => new Date(ms).toISOString();

function payoutLines(net: FlNetworkState): string[] {
  const tag = "[address book, eth_getCode and the network profile]";
  switch (net.payouts) {
    case "not-deployed":
      return [`Payouts: none to show. WorkMarket is ${NOT_AVAILABLE.toLowerCase()} ${tag}.`];
    case "dark":
      return [`Payouts: ${PAYOUTS_DARK}. ${tag}`];
    case "open":
      return [`Payouts: open on this network; read them with fl_payouts_list. ${tag}`];
    case "unknown":
      return [`Payouts: — (whether WorkMarket is on this network is unknown${net.phaseReason ? `: ${net.phaseReason}` : ""})`];
  }
}

async function history(args: Record<string, unknown>, deps: FlHistoryDeps): Promise<string> {
  const f = (typeof args.filter === "string" ? args.filter.trim() : "all") || "all";
  if (!(FILTERS as readonly string[]).includes(f)) return `fl_history_read's filter must be one of ${FILTERS.join(", ")}. Nothing was read.`;
  const filter = f as Filter;
  if (!deps.desktop) return NEEDS_DESKTOP;
  const want = (k: Filter) => filter === "all" || filter === k;
  const out: string[] = [];
  if (want("rounds") || want("consent") || want("gate")) {
    let ov: FlOverview;
    try {
      ov = await deps.overview();
    } catch (e) {
      return "couldn't read this device's FL record: " + msg(e);
    }
    const tag = "[this device, fl_rounds.json]";
    if (ov.storeError) out.push(`Core could not read its rounds file, so this may be incomplete: ${ov.storeError}`);
    if (want("rounds")) {
      out.push(`Round starts the member approved ${tag}:`);
      if (ov.starts.length === 0) out.push("- none");
      for (const s of [...ov.starts].sort((a, b) => b.authorizedAtMs - a.authorizedAtMs)) {
        out.push(
          `- ${at(s.authorizedAtMs)}: plan ${s.planHash.slice(0, 16)}…, coordinator ${s.coordinatorUrl}, work ${s.requires}${s.roundId ? `, round ${s.roundId}` : ""}${s.revokedAtMs ? `, consent withdrawn ${at(s.revokedAtMs)}` : ""}`,
        );
      }
      out.push("No training ran from these: this build has no device training worker.");
    }
    if (want("consent")) {
      const ctag = "[the device worker's consent file]";
      if (ov.consentError) out.push(`Rounds consented to: — (${ov.consentError})`);
      else out.push(`Rounds consented to ${ctag}: ${ov.consentedRounds.length ? ov.consentedRounds.join(", ") : "none"}`);
    }
    if (want("gate")) {
      out.push(`Eval gate decisions ${tag}:`);
      if (ov.gates.length === 0) out.push("- none");
      for (const g of [...ov.gates].sort((a, b) => b.decidedAtMs - a.decidedAtMs)) {
        out.push(`- ${at(g.decidedAtMs)}: adapter ${g.adapterSha256.slice(0, 16)}… ${g.decision.verdict}${g.round ? `, from round ${g.round.roundId}` : ""}`);
      }
      out.push(ov.activeAdapter ? `The local model is set to serve an adapter (${ov.activeAdapter}).` : "No adapter is served now.");
    }
  }
  if (want("payouts")) {
    try {
      out.push(...payoutLines(await deps.networkState()));
    } catch (e) {
      out.push(`Payouts: — (couldn't read which FL contracts this network has: ${msg(e)})`);
    }
  }
  return out.join("\n");
}

async function patronage(args: Record<string, unknown>, deps: FlHistoryDeps): Promise<string> {
  const group = typeof args.groupId === "string" ? args.groupId.trim() : "";
  if (group.length > 200) return "fl_patronage_read's groupId is too long. Nothing was read.";
  if (!deps.desktop) return NEEDS_DESKTOP;
  const tag = "[PatronageLedger, eth_call on chain]";
  let r: RewardInfo;
  try {
    r = await deps.reward(group);
  } catch (e) {
    return `Patronage: — (${msg(e)})`;
  }
  let wei: bigint | null = null;
  try {
    wei = BigInt(r.pendingWei);
  } catch {
    wei = null;
  }
  const lines = [
    `Patronage units recorded for this member: ${r.weight} ${tag}`,
    `Pending dividend: ${wei === null ? `— (core returned ${JSON.stringify(r.pendingWei)})` : saltText(wei, 4)} ${tag}`,
  ];
  if (group) lines.push(`Group ${group}: ${r.round === 0 ? "no round recorded on the ledger" : `${r.round} rounds recorded on the ledger`} ${tag}`);
  lines.push("Units and the dividend are per member, not per group. The ledger records units only when its settler records a round, and no dividend can be claimed from the app in this release.");
  return lines.join("\n");
}

export async function runFlHistoryTool(name: FlHistoryToolName, args: Record<string, unknown>, deps: FlHistoryDeps): Promise<string> {
  return name === "fl_history_read" ? history(args, deps) : patronage(args, deps);
}
