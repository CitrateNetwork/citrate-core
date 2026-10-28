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
import type { ActivationStatusView, ExitOutcomeView } from "../bridge/domains";
import { UNACTIVATED_WITHDRAW_NOTE } from "../shell/chainTruth";

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
  | "Unlocked · not activated"
  | `Registered · active from epoch ${number}`
  | "Validator active"
  | "Exiting"
  | "Bond returned"
  | "Exit settled · slashed"
  | "Exit settled · outcome not read"
  | "Slashed";

export type GateKey = "wallet" | "membership" | "bond" | "lock" | "synced" | "notValidator" | "key";

export interface GateCheck {
  key: GateKey;
  label: string;
  /** true met · false not met · null not read (unknown is not "no"). */
  ok: boolean | null;
  detail: string;
}

export type ActivationPhase =
  | "reading"
  | "blocked"
  | "ready"
  | "pending"
  | "failed"
  | "registered"
  | "active"
  | "exiting"
  | "exited"
  | "exitSettled"
  | "slashed";

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

/**
 * The ONE place local and public heights are compared: public head minus local height. Positive
 * means behind, negative means AHEAD (a divergent branch or a stale public RPC). `null` when
 * either height is unknown. Every sync decision goes through this (tripwire in the tests).
 */
export function syncGap(st: ActivationStatusView): number | null {
  if (st.localBlock == null || st.headBlock == null) return null;
  return st.headBlock - st.localBlock;
}

/**
 * Contract §1 "Synced": `eth_syncing` = false AND local height within 5 blocks of the public
 * head, in EITHER direction. A local node ahead of the head is not Synced.
 */
export function isNetworkSynced(st: ActivationStatusView | null): boolean | null {
  if (!st || st.localSyncing == null) return null;
  const gap = syncGap(st);
  if (gap == null) return null;
  if (st.localSyncing) return false;
  return Math.abs(syncGap(st) ?? Infinity) <= SYNC_MARGIN_BLOCKS;
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

  // Owner decision 2026-10-19 / chain#331: activation only inside the bond lock (activate() reverts
  // once MemberBond.isUnlocked() is true). An unread lock is unknown, never "locked". An activated
  // bond is never offered activation, so it has no lock check.
  if (st?.bondActivated !== true) {
    const u = st && st.bondDeployed === true ? st.bondUnlocked : null;
    checks.push({
      key: "lock",
      label: "Bond lock",
      ok: u === false ? true : u === true ? false : null,
      detail:
        u === false
          ? "Locked (MemberBond.isUnlocked() = false)."
          : u === true
            ? `Unlocked · not activated. ${UNACTIVATED_WITHDRAW_NOTE}`
            : "— not read yet (MemberBond.isUnlocked)",
    });
  }

  const synced = isNetworkSynced(st);
  const gap = st ? syncGap(st) : null;
  checks.push({
    key: "synced",
    label: "Node",
    ok: synced,
    detail:
      synced === true
        ? "Synced"
        : synced === false
          ? `Node not Synced · ${gap == null ? "—" : gap < 0 ? `${fmtInt(-gap)} blocks ahead of the network head (a different branch or a stale public RPC)` : `${fmtInt(gap)} blocks behind`} (needs to be within ${SYNC_MARGIN_BLOCKS} of the network head)`
          : "— not read yet (the local node is not answering)",
  });

  let nv: boolean | null = null;
  let nvDetail = "— not read yet (MemberBond.activated, ValidatorRegistry.validatorInfo)";
  if (st && st.bondActivated === true) {
    nv = false;
    nvDetail = "Already activated: this bond's principal is in the ValidatorRegistry.";
  } else if (st && st.bondActivated === false) {
    if (st.registryStatus === 0 || (st.registryStatus == null && !st.nodeKeyReady)) {
      // No key yet means nothing is registered under it; the "key" check below blocks activation.
      nv = true;
      nvDetail = "Not a validator";
    } else if (st.registryStatus != null) {
      nv = false;
      nvDetail = "This node's validator key is already registered in the ValidatorRegistry, so it cannot register again.";
    }
  }
  checks.push({ key: "notValidator", label: "Validator", ok: nv, detail: nvDetail });

  // Activation signs the registration with this node's proposer key, so it needs one on disk.
  if (st?.bondActivated !== true) {
    const k = st ? st.nodeKeyReady : null;
    checks.push({
      key: "key",
      label: "Validator key",
      ok: k,
      detail:
        k === true
          ? "This node has its validator key."
          : k === false
            ? "This node has no validator key yet. It mints one when block production is allowed for it."
            : "— not read yet (this node's proposer key)",
    });
  }

  return { ready: checks.every((c) => c.ok === true), checks };
}

/**
 * Contract §1 Validators (chain#319 rule): the state word, or `null` (shown as "—") when a needed
 * read is missing. `ValidatorRegistry.isActive` is already true AT REGISTRATION, before consensus
 * uses the validator, so the waiting state is keyed on `actEpoch > currentEpoch()`, and "Validator
 * active" needs `isActive` AND `actEpoch <= currentEpoch()`.
 */
export function validatorState(st: ActivationStatusView | null): { word: ValidatorWord | null; detail: string } {
  const unknown = { word: null, detail: "— not read yet" };
  if (!st) return unknown;
  if (st.registryStatus === 2) {
    // The key stays Exiting after the payout AND after a non-Byzantine slash drains it, so the
    // registry's zeros say only that the exit settled; the exit logs say how.
    if (bondReturned(st)) {
      const o = st.exitOutcome as ExitOutcomeView;
      return {
        word: "Bond returned",
        detail:
          `The exit escrow of ${fmtSaltWei(o.escrowedWei)} SALT was paid out (ValidatorRegistry Withdrawn, block ${fmtInt(o.paidOutBlock ?? 0)}), ` +
          "and no slash was logged under this key from the exit to the payout. Its registry status stays Exiting.",
      };
    }
    if (exitSettled(st)) {
      const o = st.exitOutcome;
      if (o?.outcome === "slashed") return { word: "Exit settled · slashed", detail: slashedDetail(st, o) };
      return {
        word: "Exit settled · outcome not read",
        detail:
          "The ValidatorRegistry holds no bonded stake and no escrow under this key, so the exit has settled. " +
          `How it settled, a payout or a slash, is not read${o?.reason ? `: ${o.reason}` : " yet (ValidatorRegistry exit logs)"}. Its registry status stays Exiting.`,
      };
    }
    return { word: "Exiting", detail: "ValidatorRegistry status: Exiting." };
  }
  if (st.registryStatus === 3) return { word: "Slashed", detail: "ValidatorRegistry status: Slashed." };
  if (st.bondDeployed == null) return unknown;
  if (st.bondDeployed === false) return { word: "Not a validator", detail: "No stake in the registry for this membership." };
  if (st.bondActivated == null) return unknown;
  if (st.bondActivated === false) {
    // Contract v1.2 §1: the bond word follows the lock as read; unread is "—".
    if (st.bondUnlocked === true) return { word: "Unlocked · not activated", detail: UNACTIVATED_WITHDRAW_NOTE };
    if (st.bondUnlocked !== false) return { word: null, detail: "— not read yet (MemberBond.isUnlocked)" };
    return { word: "Bond locked · not staked", detail: "Your grant sits in your bond, not in the ValidatorRegistry (MemberBond.activated = false)." };
  }
  if (st.registryStatus !== 1 || st.activationEpoch == null || st.currentEpoch == null) return unknown;
  if (st.activationEpoch > st.currentEpoch) {
    return { word: `Registered · active from epoch ${st.activationEpoch}`, detail: epochWait(st, st.activationEpoch) };
  }
  if (st.registryActive === true) {
    return {
      word: "Validator active",
      detail:
        `In the active set since epoch ${fmtInt(st.activationEpoch)} (ValidatorRegistry.isActive, activation epoch reached). ` +
        "Consensus can take up to an epoch longer to use the new set. Producing blocks, a block from this node's key, is the evidence that it takes part; " +
        "active does not imply it, and today only one validator produces.",
    };
  }
  if (st.registryActive === false) {
    return { word: null, detail: "Registered, but ValidatorRegistry.isActive is false: not in the active set." };
  }
  return unknown;
}

/**
 * The exit settled: the key is Exiting and the registry holds neither bonded stake nor escrow
 * under it. That is a payout OR a slash (a non-Byzantine slash keeps the key Exiting while it
 * draws bonded, then escrow). An unread leg is not zero, so it stays Exiting.
 */
function exitSettled(st: ActivationStatusView): boolean {
  return st.registryStatus === 2 && st.registryBondedWei === "0" && st.registryEscrowWei === "0";
}

function toWei(v: string | null | undefined): bigint | null {
  if (v == null || !/^\d+$/.test(v)) return null;
  return BigInt(v);
}

/**
 * The bond came back, proven by the registry's exit logs (exitOutcome): the exit escrowed at least
 * the bond's principal, a Withdrawn after it paid at least that, and no Slashed was logged under
 * the key before the payout. The amounts are re-checked here, so an inconsistent read is never
 * "returned".
 */
function bondReturned(st: ActivationStatusView): boolean {
  if (!exitSettled(st)) return false;
  const o = st.exitOutcome;
  if (!o || o.outcome !== "returned" || o.slashedWei != null || o.paidOutBlock == null) return false;
  const principal = toWei(st.principalWei);
  const escrowed = toWei(o.escrowedWei);
  const paid = toWei(o.paidOutWei);
  return principal != null && escrowed != null && paid != null && escrowed >= principal && paid >= escrowed;
}

/** A slashed exit in the logs' own amounts: never a claim that the bond came back. */
function slashedDetail(st: ActivationStatusView, o: ExitOutcomeView): string {
  const parts: string[] = [];
  if (o.slashedWei != null) {
    parts.push(`The ValidatorRegistry logged a slash under this key: penalties of ${fmtSaltWei(o.slashedWei)} SALT (Slashed events), drawn from stake, then escrow, then unmatured rewards.`);
  }
  const principal = toWei(st.principalWei);
  const escrowed = toWei(o.escrowedWei);
  if (principal != null && escrowed != null && escrowed < principal) {
    parts.push(`The exit escrowed ${fmtSaltWei(o.escrowedWei)} of the bond's ${fmtSaltWei(st.principalWei)} SALT principal, so stake was slashed before the exit.`);
  }
  if (o.paidOutWei != null && o.paidOutBlock != null) {
    parts.push(`A withdrawal of ${fmtSaltWei(o.paidOutWei)} SALT was logged at block ${fmtInt(o.paidOutBlock)} (Withdrawn); it can include rewards.`);
  }
  parts.push("Its registry status stays Exiting.");
  return parts.join(" ");
}

/** When the registered stake joins the active set: the real activation epoch, in blocks and measured time. */
function epochWait(st: ActivationStatusView, actEpoch: number): string {
  const len = st.epochBlocks;
  const parts = [`Your stake is registered. It becomes active at epoch ${fmtInt(actEpoch)} (the current epoch is ${fmtInt(st.currentEpoch ?? 0)}).`];
  if (len != null) {
    const start = actEpoch * len;
    if (st.headBlock != null) {
      const left = Math.max(0, start - st.headBlock);
      parts.push(`Epoch ${fmtInt(actEpoch)} starts at block ${fmtInt(start)}, ${fmtBlocksDuration(left, st.blockTimeMs)} from now.`);
    } else {
      parts.push(`Epoch ${fmtInt(actEpoch)} starts at block ${fmtInt(start)}.`);
    }
  }
  return parts.join(" ");
}

/** A warning when the bond was activated with a key that is not this node's key, else null. */
export function keyMismatch(st: ActivationStatusView | null): string | null {
  if (!st || st.bondActivated !== true || !st.bondPubkey || !st.nodePubkey) return null;
  if (st.bondPubkey.toLowerCase() === st.nodePubkey.toLowerCase()) return null;
  return (
    `Your bond is registered with a different validator key (${short(st.bondPubkey)}) than this node's (${short(st.nodePubkey)}). ` +
    "This node does not sign as your validator; the node holding that key does."
  );
}

/** The bond's principal as read, e.g. "32,000"; "—" when not read. */
export function principalSalt(st: ActivationStatusView | null): string {
  return fmtSaltWei(st?.principalWei ?? null);
}

/** The Activate button and review title: the principal as read, no amount before it is read. */
export function activateValidatorLabel(st: ActivationStatusView | null): string {
  const principal = principalSalt(st);
  return principal === "—" ? "Activate validator" : `Activate validator · bond ${principal} SALT`;
}

/** Contract §5.4 + §5.6 + §5.7: the one grant explanation, from the bond's own reads. */
export function activationGrantCopy(st: ActivationStatusView | null): string {
  const amount = fmtSaltWei(st?.principalWei ?? null);
  const date = st?.unlockTimestamp != null ? new Date(st.unlockTimestamp * 1000).toISOString().slice(0, 10) : "—";
  const block = st?.unlockBlock != null ? fmtInt(st.unlockBlock) : "—";
  const custody = `Your membership comes with a ${amount} SALT grant, locked in your bond until ${date} (block ${block}); the lock opens when either is reached.`;
  const staked =
    st && bondReturned(st)
      ? "Your validator exit has paid out and the bond is returned, so the grant is no longer staked."
      : st && exitSettled(st)
        ? "Your validator exit has settled in the ValidatorRegistry, so the grant is no longer staked there."
        : st?.bondActivated === true
        ? "Your bond is activated, so the grant is staked in the ValidatorRegistry."
        : "It is not staked until you activate the bond.";
  return [custody, staked, GRANT_DISCLAIMER, BETANET_CARRYOVER].join(" ");
}

/**
 * Which screen the step shows. A sent activation stays Pending until the chain shows the bond
 * activated. `pendingTxHash` is the step's recorded tx: while it is set and the read did not
 * report that tx's receipt, the step is Pending, never "ready" (no second activation).
 */
export function activationPhase(st: ActivationStatusView | null, walletLinked: boolean, pendingTxHash: string | null = null): ActivationPhase {
  if (!st) return pendingTxHash ? "pending" : "reading";
  if (st.registryStatus === 2) return bondReturned(st) ? "exited" : exitSettled(st) ? "exitSettled" : "exiting";
  if (st.registryStatus === 3) return "slashed";
  if (st.bondActivated === true) {
    const v = validatorState(st);
    if (v.word === "Validator active") return "active";
    if (v.word?.startsWith("Registered")) return "registered";
    return "reading";
  }
  if (st.tx && (!pendingTxHash || st.tx.hash === pendingTxHash)) {
    if (st.tx.included === true && st.tx.success === false) return "failed";
    return "pending";
  }
  if (pendingTxHash) return "pending";
  return activationGate(st, walletLinked).ready ? "ready" : "blocked";
}
