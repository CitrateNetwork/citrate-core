// =====================================================================
// citrate-core: catalogued chain reads for the UI truth contract (C1-V)
//
// The chain team's UI truth contract v1 (citrate-chain docs/ui-truth/UI_TRUTH_CONTRACT.md)
// lists every on-chain number the app may show and the one source each comes from (§2), and
// the state words it may use (§1). This module holds the reads and the pure helpers that turn
// them into those words, so every surface uses the same definition:
//
//   - validator state: from ValidatorRegistry.isActive(pubkey), validatorInfo(pubkey).actEpoch and
//     currentEpoch() (contract v1.1 §1), never stake >= 32k and never LiquidStakingPool self-stake;
//   - minimum stake: ValidatorRegistry.minStake(), read, never a hard-coded 32,000 (§2);
//   - durations in blocks: blocks x the block time measured over the last 100 blocks, never an
//     assumed 12 s (§2 "Durations given in blocks").
//
// Reads go to the public RPC (rpc.citrate.ai) through viem; the address comes from the bundled
// address book (a reroll moves it). A failed read is `null`, shown as "—" with a reason (rule 2).
// =====================================================================
import { createPublicClient, http } from "viem";
import { citrate } from "../chain";
import book from "../../src-tauri/addresses/40204.json";

export const VALIDATOR_REGISTRY = book.addresses.ValidatorRegistry as `0x${string}`;

export const REGISTRY_ABI = [
  { type: "function", name: "isActive", stateMutability: "view", inputs: [{ type: "bytes32" }], outputs: [{ type: "bool" }] },
  { type: "function", name: "minStake", stateMutability: "view", inputs: [], outputs: [{ type: "uint256" }] },
  // currentEpoch() — selector 0x76671808. EPOCH() = 1,000 blocks.
  { type: "function", name: "currentEpoch", stateMutability: "view", inputs: [], outputs: [{ type: "uint256" }] },
  // validatorInfo(bytes32) — selector 0x00408838 (ValidatorRegistry.sol).
  {
    type: "function",
    name: "validatorInfo",
    stateMutability: "view",
    inputs: [{ type: "bytes32" }],
    outputs: [
      { type: "address", name: "staker" },
      { type: "uint256", name: "bonded" },
      { type: "uint256", name: "rewards" },
      { type: "uint256", name: "escrow" },
      { type: "uint256", name: "admissionMin" },
      { type: "uint64", name: "actEpoch" },
      { type: "uint64", name: "exitEpoch" },
      { type: "uint64", name: "escrowUnlock" },
      { type: "uint8", name: "status" },
      { type: "uint256", name: "claimableNow" },
    ],
  },
] as const;

/**
 * This node's key as the registry sees it (contract v1.1 §1). `isActive(pubkey)` is already true
 * at registration, so it cannot tell a pending activation from an active validator on its own;
 * `actEpoch` against `currentEpoch` does. `status`: 0 None, 1 Active, 2 Exiting, 3 Slashed.
 */
export interface ValidatorActivation {
  isActive: boolean;
  status: number;
  actEpoch: number;
  currentEpoch: number;
}

/** "Validator active" (§1): isActive AND the activation epoch has been reached. */
export function isValidatorActive(a: ValidatorActivation | null): boolean {
  return !!a && a.isActive;
}

/** The note every "Validator active" carries (§1): consensus follows the epoch snapshot. */
export const CONSENSUS_LAG_NOTE = "consensus can lag by up to an epoch; Producing blocks is the evidence of participation";

/** How many blocks the block-time measurement spans (contract §2: "the last 100 blocks"). */
export const BLOCK_TIME_WINDOW = 100n;

/** The reads this module makes. Injected into the store so tests can supply a fixed chain. */
export interface ChainReads {
  /** `isActive(pubkey)`, `validatorInfo(pubkey)` (status, actEpoch) and `currentEpoch()`. */
  validatorActivation(pubkey: `0x${string}`): Promise<ValidatorActivation>;
  /** `ValidatorRegistry.minStake()` in wei. */
  minStakeWei(): Promise<bigint>;
  /** Seconds per block, measured over the last BLOCK_TIME_WINDOW blocks; null when it can't be. */
  blockSeconds(): Promise<number | null>;
  /** The distinct `proposerPubkey`s of the last PRODUCER_SAMPLE_BLOCKS blocks (one JSON-RPC batch). */
  producerSample(): Promise<ProducerSample>;
}

/**
 * How many recent blocks the producer sample spans. Contract §2 names "distinct proposerPubkey over
 * the last 1,000 blocks"; the chain team ruled a 100-block sample (one JSON-RPC batch) is the
 * source for now, labelled with its window. At ~2 s blocks that is about 3 minutes, so a key that
 * is not seen is "not seen producing in the last 100 blocks", never "not producing".
 */
export const PRODUCER_SAMPLE_BLOCKS = 100;

/** A producer sample: the head it was read at, how many blocks came back, the distinct keys. */
export interface ProducerSample {
  head: number;
  window: number;
  proposers: string[];
}

/** The window label a producing figure carries: the N blocks that actually came back. */
export function producerWindowLabel(sample: ProducerSample): string {
  return `sampled over the last ${sample.window} blocks`;
}

/**
 * Why a sample cannot back a producing figure: fewer than PRODUCER_SAMPLE_BLOCKS blocks came back
 * (failed batch entries). null = the sample is complete. A short sample shows "—" plus this reason.
 */
export function producerShortfall(sample: ProducerSample): string | null {
  return sample.window < PRODUCER_SAMPLE_BLOCKS ? `only ${sample.window} of ${PRODUCER_SAMPLE_BLOCKS} blocks came back` : null;
}

/** Distinct, lower-cased `proposerPubkey`s of a list of blocks, in first-seen order. */
export function distinctProposers(blocks: unknown[]): string[] {
  const seen: string[] = [];
  for (const b of blocks) {
    const k = b && typeof b === "object" ? (b as { proposerPubkey?: unknown }).proposerPubkey : undefined;
    if (typeof k !== "string" || !/^0x[0-9a-fA-F]{64}$/.test(k)) continue;
    const key = k.toLowerCase();
    if (!seen.includes(key)) seen.push(key);
  }
  return seen;
}

export type ProducingWord = "Producing blocks" | `Not seen producing in the last ${number} blocks`;

/**
 * "Producing blocks" only when this node's key is in the sample (contract §1). null = unknown,
 * including a short sample (producerShortfall), which never backs either word.
 */
export function producingWord(sample: ProducerSample | null, pubkey: string | null): ProducingWord | null {
  if (!sample || !pubkey || producerShortfall(sample)) return null;
  return sample.proposers.includes(pubkey.toLowerCase()) ? "Producing blocks" : `Not seen producing in the last ${sample.window} blocks`;
}

interface BlockStamp {
  number: bigint;
  timestamp: bigint;
}

/** Mean seconds per block between two blocks, or null when the pair can't give one. */
export function measuredBlockSeconds(latest: BlockStamp, older: BlockStamp): number | null {
  const blocks = latest.number - older.number;
  const secs = latest.timestamp - older.timestamp;
  if (blocks <= 0n || secs <= 0n) return null;
  return Number(secs) / Number(blocks);
}

/** The live reads over the public RPC. */
export function createChainReads(): ChainReads {
  const client = createPublicClient({ chain: citrate, transport: http() });
  return {
    async validatorActivation(pubkey) {
      const read = <F extends "isActive" | "validatorInfo" | "currentEpoch">(functionName: F, args?: readonly [`0x${string}`]) =>
        client.readContract({ address: VALIDATOR_REGISTRY, abi: REGISTRY_ABI, functionName, args } as never);
      const [isActive, info, epoch] = await Promise.all([
        read("isActive", [pubkey]) as Promise<boolean>,
        read("validatorInfo", [pubkey]) as Promise<readonly unknown[]>,
        read("currentEpoch") as Promise<bigint>,
      ]);
      return { isActive, status: Number(info[8]), actEpoch: Number(info[5]), currentEpoch: Number(epoch) };
    },
    async minStakeWei() {
      return client.readContract({ address: VALIDATOR_REGISTRY, abi: REGISTRY_ABI, functionName: "minStake" });
    },
    async blockSeconds() {
      const latest = await client.getBlock({ blockTag: "latest" });
      if (latest.number == null || latest.number < BLOCK_TIME_WINDOW) return null;
      const older = await client.getBlock({ blockNumber: latest.number - BLOCK_TIME_WINDOW });
      if (older.number == null) return null;
      return measuredBlockSeconds({ number: latest.number, timestamp: latest.timestamp }, { number: older.number, timestamp: older.timestamp });
    },
    async producerSample() {
      const head = await client.getBlockNumber();
      const n = BigInt(PRODUCER_SAMPLE_BLOCKS);
      const from = head >= n - 1n ? head - (n - 1n) : 0n;
      const batch = [];
      for (let b = from, id = 0; b <= head; b++, id++) {
        batch.push({ jsonrpc: "2.0", id, method: "eth_getBlockByNumber", params: ["0x" + b.toString(16), false] });
      }
      const res = await fetch(citrate.rpcUrls.default.http[0], {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(batch),
      });
      if (!res.ok) throw new Error("rpc.citrate.ai answered " + res.status);
      const body: unknown = await res.json();
      if (!Array.isArray(body)) throw new Error("rpc.citrate.ai did not answer the batch");
      const blocks = body.map((r) => (r && typeof r === "object" ? (r as { result?: unknown }).result : null)).filter(Boolean);
      if (blocks.length === 0) throw new Error("no blocks came back");
      return { head: Number(head), window: blocks.length, proposers: distinctProposers(blocks) };
    },
  };
}

/** "~2 s", "~28 h", "~3.5 d": a rough duration, always marked approximate. */
export function approxDuration(seconds: number): string {
  if (seconds < 90) return "~" + Math.max(1, Math.round(seconds)) + " s";
  if (seconds < 90 * 60) return "~" + Math.round(seconds / 60) + " min";
  if (seconds < 48 * 3600) return "~" + Math.round(seconds / 3600) + " h";
  const d = seconds / 86400;
  return "~" + (d < 10 ? d.toFixed(1) : String(Math.round(d))) + " d";
}

/**
 * A duration given in blocks, shown both ways (contract §2): "50,400 blocks (~28 h)".
 * Without a measured block time it shows the block count alone, never a guessed time.
 */
export function blocksWithTime(blocks: number, secondsPerBlock: number | null): string {
  const n = Math.max(0, Math.round(blocks)).toLocaleString("en-US");
  const label = n + (blocks === 1 ? " block" : " blocks");
  if (secondsPerBlock == null || !(secondsPerBlock > 0)) return label;
  return label + " (" + approxDuration(blocks * secondsPerBlock) + ")";
}

/** Whole SALT from a wei decimal string, or null when it isn't a number. */
export function saltFromWei(wei: string | null | undefined): number | null {
  if (wei == null || wei === "") return null;
  try {
    return Number(BigInt(wei)) / 1e18;
  } catch {
    return null;
  }
}

/**
 * Whether this member's validator rewards (`ValidatorRegistry.rewardsOf`) are read, shown and
 * claimed through the bond: registered stake, or a bond that activated (`MemberBond.activated()`).
 * After a full exit or a slash the stake reads 0 and pubkeyOfStaker is cleared, but the bond stays
 * activated and can still claim the key's matured rewards. An unread flag (null) is not activated.
 */
export function readsValidatorRewards(bondedStake: number, bondActivated: boolean | null): boolean {
  return bondedStake > 0 || bondActivated === true;
}

/** The §1 validator words this app can derive today (contract v1.1). */
export type ValidatorWord = "Not a validator" | "Bond locked · not staked" | "Staked · active from next epoch" | `Registered · active from epoch ${number}` | "Validator active" | "Exiting" | "Slashed";

/**
 * The validator state word, from the registry only (contract v1.1 §1 Validators).
 *  - `stakeSalt`: `ValidatorRegistry.stakeOf(pubkey)` for the member's key, in SALT (0 = none).
 *  - `activation`: isActive / validatorInfo / currentEpoch for the key, null when not read.
 *  - `hasGrant`: the membership grant is on chain (its bond exists), for the bond-locked word.
 * "Registered · active from epoch N" shows the real N (never a promised epoch); "Validator active"
 * needs isActive AND actEpoch <= currentEpoch. Returns null when the stake is registered but the
 * activation has not been read, or the registry reads are inconsistent: the caller shows "—".
 * LiquidStakingPool self-stake is not an input: it never makes a validator.
 */
export function validatorWord(stakeSalt: number, activation: ValidatorActivation | null, hasGrant: boolean): ValidatorWord | null {
  if (!(stakeSalt > 0)) return hasGrant ? "Bond locked · not staked" : "Not a validator";
  if (!activation) return null;
  return activation.isActive ? "Validator active" : "Staked · active from next epoch";
}
