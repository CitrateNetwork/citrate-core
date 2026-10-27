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
/** A head read still unanswered after this is a failed read (unknown), and a new read may start. */
export const HEAD_READ_TIMEOUT_MS = 20_000;
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

/** The two series the rate is measured from, each pruned to WINDOW_MS. */
export interface SyncSeries {
  local: HeightPoint[];
  head: HeightPoint[];
}

export const emptySeries = (): SyncSeries => ({ local: [], head: [] });

/**
 * Add one reading. A missing or 0 height (not read) adds nothing. A height below the last one
 * (the chain data was reset, or the profile RPC changed) restarts that series, so a rate is never
 * measured across the jump. Out-of-order or same-time readings are dropped. Points older than
 * WINDOW_MS are pruned. Returns a new series; the input is not changed.
 */
export function observe(series: SyncSeries, which: "local" | "head", t: number, v: number | null | undefined): SyncSeries {
  const prune = (pts: HeightPoint[]) => pts.filter((p) => t - p.t <= WINDOW_MS);
  let pts = series[which];
  if (v == null || !Number.isFinite(v) || v <= 0 || !Number.isFinite(t)) return { ...series, [which]: prune(pts) };
  const last = pts[pts.length - 1];
  if (last && t <= last.t) return series;
  if (last && v < last.v) pts = [];
  return { ...series, [which]: [...prune(pts), { t, v }] };
}

/** Least-squares slope in blocks per second, or null with fewer than two distinct times. */
export function slopePerSec(pts: HeightPoint[]): number | null {
  if (pts.length < 2) return null;
  const t0 = pts[0].t;
  const xs = pts.map((p) => (p.t - t0) / 1000);
  const mx = xs.reduce((a, b) => a + b, 0) / xs.length;
  const my = pts.reduce((a, p) => a + p.v, 0) / pts.length;
  let num = 0;
  let den = 0;
  for (let i = 0; i < pts.length; i++) {
    num += (xs[i] - mx) * (pts[i].v - my);
    den += (xs[i] - mx) ** 2;
  }
  return den > 0 ? num / den : null;
}

const span = (pts: HeightPoint[]) => (pts.length ? pts[pts.length - 1].t - pts[0].t : 0);

export type SyncProgress =
  | { kind: "unknown"; missing: "local" | "head" }
  | { kind: "synced"; local: number; head: number }
  | {
      kind: "syncing";
      local: number;
      head: number;
      behind: number;
      /** "estimating": not enough points yet; "catching-up": net rate positive, ETA given;
       *  "not-catching-up": measured, but the gap is not closing; "stalled": no new local block in STALL_MS. */
      phase: "estimating" | "catching-up" | "not-catching-up" | "stalled";
      /** Net catch-up rate in blocks/s; set only once measured. */
      rate: number | null;
      /** Seconds to catch up; set only in "catching-up". */
      etaSec: number | null;
    };

/** Whether the local height has not moved for STALL_MS (observed for at least that long). */
function stalledFor(local: HeightPoint[], now: number): boolean {
  const last = local[local.length - 1];
  let since = last.t;
  for (let i = local.length - 1; i >= 0 && local[i].v === last.v; i--) since = local[i].t;
  return now - since >= STALL_MS;
}

/** The sync progress at `now` from the two series. */
export function syncProgress(series: SyncSeries, now: number): SyncProgress {
  const local = series.local[series.local.length - 1];
  if (!local || now - local.t > LOCAL_FRESH_MS) return { kind: "unknown", missing: "local" };
  const head = series.head[series.head.length - 1];
  if (!head || now - head.t > HEAD_FRESH_MS) return { kind: "unknown", missing: "head" };
  const behind = head.v - local.v;
  if (behind <= SYNCED_WITHIN_BLOCKS) return { kind: "synced", local: local.v, head: head.v };
  const base = { kind: "syncing" as const, local: local.v, head: head.v, behind };
  if (stalledFor(series.local, now)) return { ...base, phase: "stalled", rate: null, etaSec: null };
  const enough =
    series.local.length >= MIN_LOCAL_POINTS &&
    series.head.length >= MIN_HEAD_POINTS &&
    span(series.local) >= MIN_SPAN_MS &&
    span(series.head) >= MIN_SPAN_MS;
  const ls = enough ? slopePerSec(series.local) : null;
  const hs = enough ? slopePerSec(series.head) : null;
  if (ls == null || hs == null) return { ...base, phase: "estimating", rate: null, etaSec: null };
  const rate = ls - hs;
  if (!(rate > MIN_NET_RATE)) return { ...base, phase: "not-catching-up", rate, etaSec: null };
  const etaSec = behind / rate;
  if (etaSec > MAX_ETA_SEC) return { ...base, phase: "not-catching-up", rate, etaSec: null };
  return { ...base, phase: "catching-up", rate, etaSec };
}

const fmt = (n: number) => Math.round(n).toLocaleString("en-US");

/** "~40 blocks/s", "~2.5 blocks/s", "~0.4 blocks/s". */
export function formatRate(rate: number): string {
  const r = rate >= 10 ? fmt(rate) : rate.toFixed(1);
  return "~" + r + " blocks/s";
}

/** "under a minute", "about 5 min", "about 3 h", "about 2 days". Always approximate. */
export function formatEta(sec: number): string {
  if (sec < 60) return "under a minute";
  if (sec < 90 * 60) return "about " + Math.round(sec / 60) + " min";
  if (sec < 48 * 3600) return "about " + Math.round(sec / 3600) + " h";
  return "about " + Math.round(sec / 86_400) + " days";
}

/** The one line a surface shows for the progress. */
export function syncLine(p: SyncProgress | null): string {
  if (!p) return "Sync unknown · not read yet";
  switch (p.kind) {
    case "unknown":
      return p.missing === "local" ? "Sync unknown · this node's height not read yet" : "Sync unknown · network head unavailable";
    case "synced":
      return "Synced";
    case "syncing": {
      const head = "Syncing · " + fmt(p.behind) + " blocks behind";
      switch (p.phase) {
        case "stalled":
          return head + " · no new blocks in 2 min, checking peers";
        case "estimating":
          return head + " · estimating…";
        case "not-catching-up":
          return head + " · not catching up yet";
        case "catching-up":
          return head + " · catching up " + formatRate(p.rate ?? 0) + " · " + formatEta(p.etaSec ?? 0);
      }
    }
  }
}

/**
 * The store's side of the window: one call per node poll. Folds this node's height (null when the
 * node isn't running, which restarts the window), starts a public head read at most every
 * HEAD_READ_MS (it lands in the window when it answers; a failed or 0x0 read adds nothing, so the
 * head goes stale and reads unknown), and returns the progress now.
 */
export class SyncTracker {
  private series: SyncSeries = emptySeries();
  private lastHeadReadAt = Number.NEGATIVE_INFINITY;

  // RED STUB (#566 review P2): the clock is accepted and not used yet.
  constructor(_clock?: () => number) {}

  fold(local: number | null, readHead: () => Promise<number | null>, now: number = Date.now()): SyncProgress {
    if (local == null) {
      this.series = emptySeries();
      this.lastHeadReadAt = Number.NEGATIVE_INFINITY;
      return syncProgress(this.series, now);
    }
    this.series = observe(this.series, "local", now, local);
    if (now - this.lastHeadReadAt >= HEAD_READ_MS) {
      this.lastHeadReadAt = now;
      Promise.resolve()
        .then(readHead)
        .then(
          (head) => {
            this.series = observe(this.series, "head", Date.now(), head);
          },
          () => {
            /* unknown, not zero: no reading is added */
          },
        );
    }
    return syncProgress(this.series, now);
  }
}

/** Where the numbers come from, for a tooltip (Rule 7). */
export const SYNC_SOURCE =
  "This node's height (local eth_blockNumber) against the public head (eth_blockNumber on the network profile's RPC). The rate is measured over the last 5 minutes; no percentage is shown.";
