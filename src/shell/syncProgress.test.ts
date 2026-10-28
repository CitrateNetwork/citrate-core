// 0.5.2 RC52 — sync progress and an honest catch-up ETA (syncProgress.ts).
// Contract §1 "Network and node": "N blocks behind", no percentage; unknown is not zero; Synced is
// within 5 blocks of a public head read in the last 60 s. The ETA is given only from a measured,
// positive net catch-up rate over enough samples.
import { describe, it, expect } from "vitest";
import {
  HEAD_FRESH_MS,
  LOCAL_FRESH_MS,
  MIN_SPAN_MS,
  STALL_MS,
  WINDOW_MS,
  emptySeries,
  formatEta,
  formatRate,
  observe,
  slopePerSec,
  syncLine,
  syncProgress,
  type SyncSeries,
} from "./syncProgress";

const T0 = 1_800_000_000_000;

/**
 * Feed `seconds` of readings: local every 2 s (the poll), head every 5 s (the throttled read).
 * localRate / headRate are blocks per second; `jitter` adds a deterministic +-j block wobble.
 */
function feed(o: { seconds: number; local0: number; head0: number; localRate: number; headRate: number; jitter?: number; start?: number; series?: SyncSeries }) {
  let s = o.series ?? emptySeries();
  const start = o.start ?? T0;
  const j = o.jitter ?? 0;
  for (let ms = 0; ms <= o.seconds * 1000; ms += 1000) {
    const t = start + ms;
    const wob = j ? ((ms / 1000) % 3) - 1 : 0; // -1, 0, +1 repeating
    if (ms % 2000 === 0) s = observe(s, "local", t, Math.round(o.local0 + o.localRate * (ms / 1000) + wob * j));
    if (ms % 5000 === 0) s = observe(s, "head", t, Math.round(o.head0 + o.headRate * (ms / 1000)));
  }
  return { series: s, now: start + o.seconds * 1000 };
}

describe("rate math: net catch-up = local slope minus head slope, over the window", () => {
  it("measures ~40 blocks/s net when the node applies 40.5/s and the head grows 0.5/s", () => {
    const { series, now } = feed({ seconds: 120, local0: 100_000, head0: 130_000, localRate: 40.5, headRate: 0.5 });
    const p = syncProgress(series, now);
    expect(p.kind).toBe("syncing");
    if (p.kind !== "syncing") return;
    expect(p.phase).toBe("catching-up");
    expect(p.rate).toBeGreaterThan(39.5);
    expect(p.rate).toBeLessThan(40.5);
    // behind = head - local at the latest readings; ETA = behind / net rate.
    expect(p.behind).toBe(p.head - p.local);
    expect(p.etaSec).toBeCloseTo(p.behind / (p.rate as number), 6);
  });

  it("smooths a jittery local height (least squares, not first-to-last)", () => {
    const { series, now } = feed({ seconds: 180, local0: 50_000, head0: 60_000, localRate: 20, headRate: 0.5, jitter: 30 });
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.rate).toBeGreaterThan(18.5);
    expect(p.rate).toBeLessThan(20.5);
  });

  it("slopePerSec is a least-squares fit in blocks per second", () => {
    expect(slopePerSec([{ t: T0, v: 10 }, { t: T0 + 10_000, v: 110 }])).toBeCloseTo(10, 9);
    expect(slopePerSec([{ t: T0, v: 10 }])).toBeNull();
    expect(slopePerSec([])).toBeNull();
  });

  it("keeps only the last WINDOW_MS of readings, so an old fast burst no longer counts", () => {
    // 4 min fast, then 6 min of the node only keeping pace with the head.
    let { series } = feed({ seconds: 240, local0: 10_000, head0: 40_000, localRate: 100, headRate: 0.5 });
    const fast = series.local[series.local.length - 1].v;
    ({ series } = feed({ seconds: 360, local0: fast + 1, head0: 40_000 + 120 + 1, localRate: 0.5, headRate: 0.5, start: T0 + 241_000, series }));
    const now = T0 + 241_000 + 360_000;
    for (const pt of [...series.local, ...series.head]) expect(now - pt.t).toBeLessThanOrEqual(WINDOW_MS);
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("not-catching-up");
    expect(p.etaSec).toBeNull();
  });
});

describe("ETA only when it means something", () => {
  it("says estimating until both series have enough points over enough time", () => {
    const { series, now } = feed({ seconds: 10, local0: 1_000, head0: 9_000, localRate: 40, headRate: 0.5 });
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("estimating");
    expect(p.rate).toBeNull();
    expect(p.etaSec).toBeNull();
    expect(syncLine(p)).toBe("Syncing · " + (p.behind).toLocaleString("en-US") + " blocks behind · estimating…");
  });

  it("gives a rate once the span reaches MIN_SPAN_MS", () => {
    const { series, now } = feed({ seconds: MIN_SPAN_MS / 1000, local0: 1_000, head0: 9_000, localRate: 40, headRate: 0.5 });
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("catching-up");
  });

  it("no ETA when the node only keeps pace with the head (net rate ~0)", () => {
    const { series, now } = feed({ seconds: 120, local0: 1_000, head0: 9_000, localRate: 0.5, headRate: 0.5 });
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("not-catching-up");
    expect(p.etaSec).toBeNull();
    expect(syncLine(p)).toMatch(/· not catching up yet$/);
    expect(syncLine(p)).not.toMatch(/about|blocks\/s/);
  });

  it("no ETA when the head pulls away (net rate negative)", () => {
    const { series, now } = feed({ seconds: 120, local0: 1_000, head0: 9_000, localRate: 0.3, headRate: 0.5 });
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("not-catching-up");
    expect(p.etaSec).toBeNull();
    expect(p.rate).toBeLessThan(0);
  });

  it("no ETA past MAX_ETA_SEC (a trickle is not catching up)", () => {
    const { series, now } = feed({ seconds: 120, local0: 1_000, head0: 9_000_000, localRate: 0.52, headRate: 0.5 });
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.etaSec).toBeNull();
  });

  it("formats the owner's example line", () => {
    const line = syncLine({ kind: "syncing", local: 100_000, head: 112_340, behind: 12_340, phase: "catching-up", rate: 41.1, etaSec: 300 });
    expect(line).toBe("Syncing · 12,340 blocks behind · catching up ~41 blocks/s · about 5 min");
  });

  it("formats rates and ETAs approximately", () => {
    expect(formatRate(40.4)).toBe("~40 blocks/s");
    expect(formatRate(2.46)).toBe("~2.5 blocks/s");
    expect(formatRate(0.42)).toBe("~0.4 blocks/s");
    expect(formatEta(20)).toBe("under a minute");
    expect(formatEta(300)).toBe("about 5 min");
    expect(formatEta(3 * 3600)).toBe("about 3 h");
    expect(formatEta(3 * 86_400)).toBe("about 3 days");
  });
});

describe("stall: no new local block for STALL_MS (contract §1 Sync stalled, 60 s)", () => {
  it("is stalled after STALL_MS with the height frozen, and says so without alarm", () => {
    const { series, now } = feed({ seconds: STALL_MS / 1000, local0: 5_000, head0: 9_000, localRate: 0, headRate: 0.5 });
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("stalled");
    expect(p.etaSec).toBeNull();
    expect(STALL_MS).toBe(60_000);
    expect(syncLine(p)).toBe("Sync stalled · " + p.behind.toLocaleString("en-US") + " blocks behind");
    expect(syncLine(p)).not.toMatch(/error|fail|stuck|broken/i);
  });

  it("is not stalled one poll short of STALL_MS", () => {
    const { series, now } = feed({ seconds: STALL_MS / 1000 - 2, local0: 5_000, head0: 9_000, localRate: 0, headRate: 0.5 });
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).not.toBe("stalled");
  });

  it("clears as soon as the local height moves again", () => {
    let { series } = feed({ seconds: 150, local0: 5_000, head0: 9_000, localRate: 0, headRate: 0.5 });
    const now = T0 + 152_000;
    series = observe(series, "local", now, 5_200);
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).not.toBe("stalled");
  });
});

describe("unknown is not zero: without both heights there is no behind, rate or ETA", () => {
  it("no readings at all is unknown (local)", () => {
    expect(syncProgress(emptySeries(), T0)).toEqual({ kind: "unknown", missing: "local" });
  });

  it("a 0x0 / 0 / null / NaN height is not a reading", () => {
    let s = emptySeries();
    s = observe(s, "local", T0, 0);
    s = observe(s, "local", T0 + 1, null);
    s = observe(s, "local", T0 + 2, Number.NaN);
    s = observe(s, "head", T0 + 3, 0);
    s = observe(s, "head", T0 + 4, undefined);
    expect(s.local).toHaveLength(0);
    expect(s.head).toHaveLength(0);
  });

  it("the head never read is unknown (head), even with a long local series", () => {
    let s = emptySeries();
    for (let i = 0; i <= 60; i++) s = observe(s, "local", T0 + i * 2000, 1_000 + i * 80);
    const p = syncProgress(s, T0 + 120_000);
    expect(p).toEqual({ kind: "unknown", missing: "head" });
    expect(syncLine(p)).toBe("Sync unknown · network head unavailable");
  });

  it("a head older than HEAD_FRESH_MS is unknown, not Synced or Syncing", () => {
    const { series, now } = feed({ seconds: 60, local0: 1_000, head0: 9_000, localRate: 40, headRate: 0.5 });
    // No head reading after `now`; the local series keeps going.
    let s = series;
    for (let ms = 2000; ms <= HEAD_FRESH_MS + 4000; ms += 2000) s = observe(s, "local", now + ms, 3_400 + ms / 25);
    expect(syncProgress(s, now + HEAD_FRESH_MS + 4000)).toEqual({ kind: "unknown", missing: "head" });
  });

  it("a local height older than LOCAL_FRESH_MS is unknown (local)", () => {
    const { series, now } = feed({ seconds: 60, local0: 1_000, head0: 9_000, localRate: 40, headRate: 0.5 });
    expect(syncProgress(series, now + LOCAL_FRESH_MS + 1)).toEqual({ kind: "unknown", missing: "local" });
  });

  it("syncLine(null) (before the first poll) is unknown, with no number", () => {
    expect(syncLine(null)).toBe("Sync unknown · not read yet");
  });
});

describe("Synced is the K3 rule: within 5 blocks of a fresh public head", () => {
  it("is Synced at exactly 5 behind, and Syncing at 6", () => {
    let s = emptySeries();
    s = observe(s, "local", T0, 1_000);
    s = observe(s, "head", T0, 1_005);
    expect(syncProgress(s, T0).kind).toBe("synced");
    expect(syncLine(syncProgress(s, T0))).toBe("Synced");
    s = observe(s, "head", T0 + 1, 1_006);
    expect(syncProgress(s, T0 + 1).kind).toBe("syncing");
  });

  it("a local height above the public head is Synced, not a negative behind", () => {
    let s = emptySeries();
    s = observe(s, "local", T0, 1_010);
    s = observe(s, "head", T0, 1_000);
    expect(syncProgress(s, T0).kind).toBe("synced");
  });
});

describe("adversarial series", () => {
  it("a local height that drops (chain data reset) restarts the local series, so no rate across the jump", () => {
    let { series } = feed({ seconds: 120, local0: 80_000, head0: 90_000, localRate: 40, headRate: 0.5 });
    const now = T0 + 122_000;
    series = observe(series, "local", now, 10);
    expect(series.local).toEqual([{ t: now, v: 10 }]);
    const p = syncProgress(series, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("estimating");
    expect(p.etaSec).toBeNull();
  });

  it("a head that drops (profile RPC changed) restarts the head series", () => {
    let { series } = feed({ seconds: 120, local0: 1_000, head0: 90_000, localRate: 40, headRate: 0.5 });
    series = observe(series, "head", T0 + 121_000, 50_000);
    expect(series.head).toEqual([{ t: T0 + 121_000, v: 50_000 }]);
  });

  it("a same-time reading is dropped; an earlier time (the clock went back) restarts the series", () => {
    let s = emptySeries();
    s = observe(s, "local", T0 + 10, 100);
    s = observe(s, "local", T0 + 10, 200);
    expect(s.local).toEqual([{ t: T0 + 10, v: 100 }]);
    s = observe(s, "local", T0 + 5, 300);
    expect(s.local).toEqual([{ t: T0 + 5, v: 300 }]);
  });

  it("observe never mutates its input", () => {
    const s = emptySeries();
    const s2 = observe(s, "local", T0, 100);
    expect(s.local).toHaveLength(0);
    expect(s2.local).toHaveLength(1);
  });
});
