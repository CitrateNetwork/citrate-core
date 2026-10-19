// =====================================================================
// citrate-core — Join-8: the "Activate validator" onboarding step's logic.
//
// Pure functions over one chain read (`bridge.node.activationStatus` → ActivationStatusView), so
// the gate, the state words and the durations are testable without a node. Every word comes from
// the UI truth contract v1 (chain team): §1 Validators and Membership-and-bond, §2 durations
// (blocks × MEASURED block time, never 12 s), §5 money copy (one grant explanation, "staked" only
// after MemberBond.activated(), no earning or projections).
//
// Grant copy: core's single grant explanation (src/data/grantCopy.ts) is in PR #349 and not on
// main yet. `activationGrantCopy` uses the contract §5.4 wording directly, with the same
// disclaimer and betanet lines, and should fold into grantCopy.ts once that lands.
// =====================================================================
import type { ActivationStatusView } from "../bridge/domains";

/** Contract §1 "Synced": local `eth_blockNumber` within this many blocks of the public head. */
export const SYNC_MARGIN_BLOCKS = 5;

/** §5.7, on every surface that mentions the grant. */
export const GRANT_DISCLAIMER = "The grant is not purchased, and it is not an investment.";
/** §5.6, until the C2 genesis spec decides what carries over. */
export const BETANET_CARRYOVER = "Betanet. What carries over to mainnet is set in the genesis spec.";

/** What the step does, in the contract's words. */
export const ACTIVATE_EXPLANATION =
  "Activating sends one transaction from your wallet to your bond (MemberBond.activate). The bond moves its principal " +
  "into the ValidatorRegistry under this node's validator key, which the node signs with. Nothing is signed until you " +
  "approve it in the Signature Ceremony. Once included, your stake is registered and your validator joins the active " +
  "set at an epoch snapshot.";

export type ValidatorWord =
  | "Not a validator"
  | "Bond locked · not staked"
  | "Staked · active from next epoch"
  | "Validator active"
  | "Exiting"
  | "Slashed";

export type GateKey = "wallet" | "membership" | "bond" | "synced" | "notValidator";

export interface GateCheck {
  key: GateKey;
  label: string;
  /** true met · false not met · null not read (unknown is not "no"). */
  ok: boolean | null;
  detail: string;
}

export type ActivationPhase = "reading" | "blocked" | "ready" | "pending" | "failed" | "staked" | "active" | "exiting" | "slashed";

const fmtInt = (n: number) => Math.round(n).toLocaleString("en-US");
const short = (h: string) => (h.length > 12 ? h.slice(0, 6) + "…" + h.slice(-4) : h);

function fmtSaltWei(wei: string | null): string {
  if (wei == null) return "—";
  try {
    return (BigInt(wei) / 10n ** 18n).toLocaleString("en-US");
  } catch {
    return "—";
  }
}

/** A duration in ms as "~N s / min / h / d". */
export function fmtDuration(ms: number): string {
  const s = ms / 1000;
  if (s < 90) return `~${Math.round(s)} s`;
  if (s < 90 * 60) return `~${Math.round(s / 60)} min`;
  if (s < 48 * 3600) return `~${Math.round(s / 3600)} h`;
  return `~${Math.round(s / 86400)} d`;
}

/** Contract §2: "50,400 blocks (~28 h)". Blocks × the measured block time; never an assumed 12 s. */
export function fmtBlocksDuration(blocks: number, blockTimeMs: number | null): string {
  const b = `${fmtInt(blocks)} blocks`;
  if (blockTimeMs == null || !(blockTimeMs > 0)) return `${b} (time not measured yet)`;
  return `${b} (${fmtDuration(blocks * blockTimeMs)})`;
}

/** Contract §1 "Synced": `eth_syncing` = false AND local height within 5 blocks of the public head. */
export function isNetworkSynced(st: ActivationStatusView | null): boolean | null {
  if (!st || st.localSyncing == null || st.localBlock == null || st.headBlock == null) return null;
  if (st.localSyncing) return false;
  return st.headBlock - st.localBlock <= SYNC_MARGIN_BLOCKS;
}

function blocksBehind(st: ActivationStatusView): number | null {
  if (st.localBlock == null || st.headBlock == null) return null;
  return Math.max(0, st.headBlock - st.localBlock);
}

/** The step's precondition gate. Opens only when every check is `true`. */
export function activationGate(st: ActivationStatusView | null, walletLinked: boolean): { ready: boolean; checks: GateCheck[] } {
  const checks: GateCheck[] = [];
  checks.push({
    key: "wallet",
    label: "Member wallet on this device",
    ok: walletLinked,
    detail: walletLinked
      ? "This device's wallet is your member wallet."
      : "This device's wallet is not linked to your membership. The bond accepts activation only from the member wallet, so link it first.",
  });

  const m = st?.membershipActive ?? null;
  checks.push({
    key: "membership",
    label: "Membership",
    ok: m,
    detail: m === true ? "Member" : m === false ? "Membership lapsed" : "— not read yet (CitrateMemberSBT.isActive)",
  });

  const b = st?.bondDeployed ?? null;
  checks.push({
    key: "bond",
    label: "Bond",
    ok: b,
    detail:
      b === true
        ? `Bond deployed · ${short(st?.bondAddress ?? "")}`
        : b === false
          ? "No bond on chain yet. The membership grant deploys it, and it has not landed."
          : "— not read yet (MembershipStakeVault.bondOf)",
  });

  const synced = isNetworkSynced(st);
  const behind = st ? blocksBehind(st) : null;
  checks.push({
    key: "synced",
    label: "Node",
    ok: synced,
    detail:
      synced === true
        ? "Synced"
        : synced === false
          ? `Node not Synced · ${behind == null ? "—" : fmtInt(behind)} blocks behind (needs to be within ${SYNC_MARGIN_BLOCKS} of the network head)`
          : "— not read yet (the local node is not answering)",
  });

  let nv: boolean | null = null;
  let nvDetail = "— not read yet (MemberBond.activated, ValidatorRegistry.validatorInfo)";
  if (st && st.bondActivated === true) {
    nv = false;
    nvDetail = "Already activated: this bond's principal is in the ValidatorRegistry.";
  } else if (st && st.bondActivated === false) {
    if (st.registryStatus === 0 || (st.registryStatus == null && !st.nodeKeyReady)) {
      nv = true;
      nvDetail = "Not a validator";
    } else if (st.registryStatus != null) {
      nv = false;
      nvDetail = "This node's validator key is already registered in the ValidatorRegistry, so it cannot register again.";
    }
  }
  checks.push({ key: "notValidator", label: "Validator", ok: nv, detail: nvDetail });

  return { ready: checks.every((c) => c.ok === true), checks };
}

/** Contract §1 Validators: the state word, or `null` (shown as "—") when a needed read is missing. */
export function validatorState(st: ActivationStatusView | null): { word: ValidatorWord | null; detail: string } {
  const unknown = { word: null, detail: "— not read yet" };
  if (!st) return unknown;
  if (st.registryStatus === 2) return { word: "Exiting", detail: "ValidatorRegistry status: Exiting." };
  if (st.registryStatus === 3) return { word: "Slashed", detail: "ValidatorRegistry status: Slashed." };
  if (st.bondDeployed == null) return unknown;
  if (st.bondDeployed === false) return { word: "Not a validator", detail: "No stake in the registry for this membership." };
  if (st.bondActivated == null) return unknown;
  if (st.bondActivated === false) {
    return { word: "Bond locked · not staked", detail: "Your grant sits in your bond, not in the ValidatorRegistry (MemberBond.activated = false)." };
  }
  if (st.registryStatus !== 1 || st.registryActive == null) return unknown;
  if (st.registryActive) {
    return {
      word: "Validator active",
      detail:
        "In the active set (ValidatorRegistry.isActive). Active is not the same as producing blocks: today only one validator produces.",
    };
  }
  return { word: "Staked · active from next epoch", detail: epochWait(st) };
}

/** When the registered stake joins the active set, in blocks and measured time. */
function epochWait(st: ActivationStatusView): string {
  const len = st.epochBlocks;
  const parts = ["Your stake is registered. The active set changes at epoch snapshots"];
  if (len != null) parts[0] += `; an epoch is ${fmtBlocksDuration(len, st.blockTimeMs)}`;
  parts[0] += ".";
  if (len != null && st.activationEpoch != null && st.activationEpoch > 0 && st.headBlock != null) {
    const start = st.activationEpoch * len;
    const left = Math.max(0, start - st.headBlock);
    parts.push(`Your activation epoch is ${fmtInt(st.activationEpoch)}, which starts at block ${fmtInt(start)}, ${fmtBlocksDuration(left, st.blockTimeMs)} from now.`);
  }
  return parts.join(" ");
}

/** Contract §5.4 + §5.6 + §5.7: the one grant explanation, from the bond's own reads. */
export function activationGrantCopy(st: ActivationStatusView | null): string {
  const amount = fmtSaltWei(st?.principalWei ?? null);
  const date = st?.unlockTimestamp != null ? new Date(st.unlockTimestamp * 1000).toISOString().slice(0, 10) : "—";
  const block = st?.unlockBlock != null ? fmtInt(st.unlockBlock) : "—";
  const custody = `Your membership comes with a ${amount} SALT grant, locked in your bond until ${date} (block ${block}); the lock opens when either is reached.`;
  const staked =
    st?.bondActivated === true
      ? "Your bond is activated, so the grant is staked in the ValidatorRegistry."
      : "It is not staked until you activate the bond.";
  return [custody, staked, GRANT_DISCLAIMER, BETANET_CARRYOVER].join(" ");
}

/** Which screen the step shows. A sent activation stays Pending until the chain shows the bond activated. */
export function activationPhase(st: ActivationStatusView | null, walletLinked: boolean): ActivationPhase {
  if (!st) return "reading";
  if (st.registryStatus === 2) return "exiting";
  if (st.registryStatus === 3) return "slashed";
  if (st.bondActivated === true) {
    if (st.registryStatus === 1 && st.registryActive != null) return st.registryActive ? "active" : "staked";
    return "reading";
  }
  if (st.tx) {
    if (st.tx.included === true && st.tx.success === false) return "failed";
    return "pending";
  }
  return activationGate(st, walletLinked).ready ? "ready" : "blocked";
}
