// =====================================================================
// citrate-core: sync progress and an honest catch-up ETA (0.5.2 RC52, UI truth contract §1
// "Network and node").
//
// One pure module turns two height series into what a member sees while the node catches up:
// how far behind it is, how fast it is closing the gap, and roughly how long that takes. Data
// sources (Rule 7):
//   - local applied height: eth_blockNumber on this node's RPC, via `node_status` (node.rs),
//     folded by Store.refreshNode every 2 s;
//   - public head: eth_blockNumber on the active network profile's RPC (ChainReads.publicHead,
//     the same profile RPC the Node page reads for "Synced").
// Rules this module keeps:
//   - Unknown is not zero. eth_blockNumber answers 0x0 on error, so a 0 or missing height is "not
//     read", never block 0. Without BOTH heights there is no "behind", no rate and no ETA.
//   - No percentage. The contract shows "N blocks behind", never "% from start".
//   - The rate is measured, never assumed: the local height's slope minus the public head's slope,
//     each a least-squares fit over the last WINDOW_MS. An ETA is given only when that net rate is
//     positive and both series have enough points over enough time; otherwise the line says
//     "estimating…" or "not catching up yet".
//   - "Synced" is the same rule as Node › Status (K3): within SYNCED_WITHIN_BLOCKS of a public
//     head read in the last HEAD_FRESH_MS.
// =====================================================================

/** Contract §1: Synced means within this many blocks of the public RPC head. */
export const SYNCED_WITHIN_BLOCKS = 5;
/** Contract §1: a public head older than this is stale, so unknown. */
export const HEAD_FRESH_MS = 60_000;
/** How often the store reads the public head while the node runs (one eth_blockNumber). */
export const HEAD_READ_MS = 5_000;
/** A local height older than this is stale (the poll ticks every 2 s). */
export const LOCAL_FRESH_MS = 15_000;
/** The sliding window the catch-up rate is measured over. */
export const WINDOW_MS = 5 * 60_000;
/** Fewest points per series before a rate is given. */
export const MIN_LOCAL_POINTS = 5;
export const MIN_HEAD_POINTS = 3;
/** Shortest span per series before a rate is given. */
export const MIN_SPAN_MS = 30_000;
/** No new local block for this long while behind reads as "no new blocks in 2 min". */
export const STALL_MS = 2 * 60_000;
/** A net rate at or below this (blocks/s) is not catching up. */
export const MIN_NET_RATE = 0.01;
/** Past this the ETA is not worth a number. */
export const MAX_ETA_SEC = 30 * 86_400;

/** One height reading at the time it was read (epoch ms). */
export interface HeightPoint {
  t: number;
  v: number;
}

/** The two series the rate is measured from. */
export interface SyncSeries {
  local: HeightPoint[];
  head: HeightPoint[];
}

export const emptySeries = (): SyncSeries => ({ local: [], head: [] });

// RED STUB: keeps the old behaviour (no progress is measured). Replaced by the implementation.
export function observe(series: SyncSeries, _which: "local" | "head", _t: number, _v: number | null | undefined): SyncSeries {
  return series;
}

export function slopePerSec(_pts: HeightPoint[]): number | null {
  return null;
}

export type SyncProgress =
  | { kind: "unknown"; missing: "local" | "head" }
  | { kind: "synced"; local: number; head: number }
  | {
      kind: "syncing";
      local: number;
      head: number;
      behind: number;
      phase: "estimating" | "catching-up" | "not-catching-up" | "stalled";
      rate: number | null;
      etaSec: number | null;
    };

export function syncProgress(_series: SyncSeries, _now: number): SyncProgress {
  return { kind: "unknown", missing: "local" };
}

export function formatRate(rate: number): string {
  return String(rate);
}

export function formatEta(sec: number): string {
  return String(sec);
}

export function syncLine(_p: SyncProgress | null): string {
  return "";
}

export const SYNC_SOURCE = "";
