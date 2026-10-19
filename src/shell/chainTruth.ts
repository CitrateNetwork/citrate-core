// =====================================================================
// citrate-core: catalogued chain reads for the UI truth contract (C1-V)
//
// The chain team's UI truth contract v1 (citrate-chain docs/ui-truth/UI_TRUTH_CONTRACT.md)
// lists every on-chain number the app may show and the one source each comes from (§2), and
// the state words it may use (§1). This module holds the reads and the pure helpers that turn
// them into those words, so every surface uses the same definition:
//
//   - validator state: from ValidatorRegistry.isActive(pubkey) and stakeOf, never stake >= 32k
//     and never LiquidStakingPool self-stake (§1 Validators);
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

const REGISTRY_ABI = [
  { type: "function", name: "isActive", stateMutability: "view", inputs: [{ type: "bytes32" }], outputs: [{ type: "bool" }] },
  { type: "function", name: "minStake", stateMutability: "view", inputs: [], outputs: [{ type: "uint256" }] },
] as const;

/** How many blocks the block-time measurement spans (contract §2: "the last 100 blocks"). */
export const BLOCK_TIME_WINDOW = 100n;

/** The reads this module makes. Injected into the store so tests can supply a fixed chain. */
export interface ChainReads {
  /** `ValidatorRegistry.isActive(pubkey)`. */
  isActive(pubkey: `0x${string}`): Promise<boolean>;
  /** `ValidatorRegistry.minStake()` in wei. */
  minStakeWei(): Promise<bigint>;
  /** Seconds per block, measured over the last BLOCK_TIME_WINDOW blocks; null when it can't be. */
  blockSeconds(): Promise<number | null>;
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
    async isActive(pubkey) {
      return client.readContract({ address: VALIDATOR_REGISTRY, abi: REGISTRY_ABI, functionName: "isActive", args: [pubkey] });
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

/** The §1 validator words this app can derive today. */
export type ValidatorWord = "Not a validator" | "Bond locked · not staked" | "Staked · active from next epoch" | "Validator active";

/**
 * The validator state word, from the registry only (§1 Validators).
 *  - `stakeSalt`: `ValidatorRegistry.stakeOf(pubkey)` for the member's key, in SALT (0 = none).
 *  - `active`: `ValidatorRegistry.isActive(pubkey)`, null when not read.
 *  - `hasGrant`: the membership grant is on chain (its bond exists), for the bond-locked word.
 * Returns null when the stake is registered but `isActive` has not been read: the caller shows
 * "—" rather than guess. LiquidStakingPool self-stake is not an input: it never makes a validator.
 */
export function validatorWord(stakeSalt: number, active: boolean | null, hasGrant: boolean): ValidatorWord | null {
  if (!(stakeSalt > 0)) return hasGrant ? "Bond locked · not staked" : "Not a validator";
  if (active === null) return null;
  return active ? "Validator active" : "Staked · active from next epoch";
}
