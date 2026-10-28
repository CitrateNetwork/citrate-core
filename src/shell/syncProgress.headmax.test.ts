// #569 Codex P2: the public head's noise rule (a small drop is held at the max) must not hold an old
// max forever. The max keeps the time it was really read; replacements never refresh it, so after a
// rollback or reset to a lower head, sustained lower readings are accepted and the series restarts.
import { describe, it, expect } from "vitest";
import {
  HEAD_FRESH_MS,
  HEAD_READ_MS,
  STALL_MS,
  WINDOW_MS,
  emptySeries,
  observe,
  syncLine,
  syncProgress,
  type SyncSeries,
} from "./syncProgress";

const T0 = 1_800_000_000_000;

/** A node caught up at ~500,000 for `sec` seconds: local every 2 s, head every 5 s, 1 block / 10 s. */
function caughtUpAt500k(sec: number): SyncSeries {
  let s = emptySeries();
  for (let i = 0; i <= sec; i++) {
    const t = T0 + i * 1000;
    if (i % 2 === 0) s = observe(s, "local", t, 499_998 + Math.floor(i / 10));
    if (i % 5 === 0) s = observe(s, "head", t, 500_000 + Math.floor(i / 10));
  }
  return s;
}

/** After the rollback: both heights at `base` growing 1 block / 10 s, from `from` for `sec` seconds. */
function afterRollback(s: SyncSeries, from: number, sec: number, base: number): { s: SyncSeries; t: number } {
  let t = from;
  for (let i = 1; i <= sec; i++) {
    t = from + i * 1000;
    if (i % 2 === 0) s = observe(s, "local", t, base + Math.floor(i / 10));
    if (i % 5 === 0) s = observe(s, "head", t, base + Math.floor(i / 10));
  }
  return { s, t };
}

describe("#569 P2: a held head max expires on its real read time", () => {
  it("a sustained lower head (500,000 -> 496,000) is accepted after the max goes stale, and the head series restarts", () => {
    let s = caughtUpAt500k(60);
    expect(syncProgress(s, T0 + 60_000).kind).toBe("synced");
    const maxV = s.head[s.head.length - 1].v;
    const dropAt = T0 + 60_000;
    // 4,000 blocks is under 1% of 500,000, so each reading on its own looks like noise.
    const r = afterRollback(s, dropAt, Math.ceil((WINDOW_MS + 60_000) / 1000), 496_000);
    s = r.s;
    const p = syncProgress(s, r.t);
    expect(p.kind).toBe("synced");
    expect(syncLine(p)).toBe("Synced");
    // The head series restarted after the drop: no point carries the old max or predates the drop.
    expect(s.head.length).toBeGreaterThan(0);
    for (const pt of s.head) {
      expect(pt.v).toBeLessThan(maxV);
      expect(pt.t).toBeGreaterThan(dropAt);
    }
  });

  it("the lower head is accepted once the max is older than HEAD_FRESH_MS (a stale head is unknown anyway)", () => {
    let s = caughtUpAt500k(60);
    const lastRealMax = s.head[s.head.length - 1].t;
    const r = afterRollback(s, T0 + 60_000, Math.ceil((HEAD_FRESH_MS + 2 * HEAD_READ_MS) / 1000), 496_000);
    s = r.s;
    expect(r.t - lastRealMax).toBeGreaterThan(HEAD_FRESH_MS);
    const p = syncProgress(s, r.t);
    expect(p.kind).toBe("synced");
    expect(s.head[s.head.length - 1].v).toBeLessThan(497_000);
  });

  it("a caught-up node after a rollback never reads 'Sync stalled' or thousands behind once the max is stale", () => {
    let s = caughtUpAt500k(60);
    const dropAt = T0 + 60_000;
    let t = dropAt;
    // The node re-applies nothing new (a quiet chain): local and head both sit at 496,000.
    for (let i = 1; i <= (WINDOW_MS * 2) / 1000; i++) {
      t = dropAt + i * 1000;
      if (i % 2 === 0) s = observe(s, "local", t, 496_000);
      if (i % 5 === 0) s = observe(s, "head", t, 496_000);
      if (t - dropAt > HEAD_FRESH_MS + HEAD_READ_MS && t - dropAt > STALL_MS) {
        const line = syncLine(syncProgress(s, t));
        expect(line).toBe("Synced");
      }
    }
  });

  it("a full reset to a much lower head (19,000 -> 50) is accepted at once", () => {
    let s = emptySeries();
    for (let i = 0; i <= 60; i += 5) {
      s = observe(s, "local", T0 + i * 1000, 19_000);
      s = observe(s, "head", T0 + i * 1000, 19_000);
    }
    const t = T0 + 65_000;
    s = observe(s, "local", t, 50);
    s = observe(s, "head", t, 50);
    expect(s.head).toEqual([{ t, v: 50 }]);
    expect(s.headMax).toEqual({ t, v: 50 });
    expect(syncProgress(s, t).kind).toBe("synced");
  });

  it("brief alternating-backend noise is still held at the max (one backend 4 behind, for 10 min)", () => {
    let s = emptySeries();
    for (let i = 0; i <= 600; i += 5) {
      const real = 300_000 + Math.floor(i / 10);
      s = observe(s, "head", T0 + i * 1000, (i / 5) % 2 === 1 ? real - 4 : real);
    }
    // Every point is the real max at that time: the lagging backend never shows through.
    for (let k = 1; k < s.head.length; k++) expect(s.head[k].v).toBeGreaterThanOrEqual(s.head[k - 1].v);
    expect(s.head.length).toBeGreaterThan(WINDOW_MS / 5000 - 2);
  });

  it("a lagging-only stretch shorter than HEAD_FRESH_MS is still held at the max", () => {
    let s = observe(emptySeries(), "head", T0, 300_000);
    for (let i = 5; i < HEAD_FRESH_MS / 1000; i += 5) s = observe(s, "head", T0 + i * 1000, 299_996);
    expect(s.head.every((p) => p.v === 300_000)).toBe(true);
    expect(s.head[0]).toEqual({ t: T0, v: 300_000 });
  });

  it("keeps the max's real read time; held replacements never refresh it", () => {
    let s = observe(emptySeries(), "head", T0, 500_000);
    s = observe(s, "head", T0 + 5_000, 499_996);
    s = observe(s, "head", T0 + 10_000, 499_995);
    expect(s.headMax).toEqual({ t: T0, v: 500_000 });
    // A real reading at or above the max moves it, with its own time.
    s = observe(s, "head", T0 + 15_000, 500_001);
    expect(s.headMax).toEqual({ t: T0 + 15_000, v: 500_001 });
    // A failed read does not touch it.
    s = observe(s, "head", T0 + 20_000, null);
    expect(s.headMax).toEqual({ t: T0 + 15_000, v: 500_001 });
  });

  it("observe does not mutate the held max of its input", () => {
    const a = observe(emptySeries(), "head", T0, 500_000);
    const b = observe(a, "head", T0 + 5_000, 500_010);
    expect(a.headMax).toEqual({ t: T0, v: 500_000 });
    expect(b.headMax).toEqual({ t: T0 + 5_000, v: 500_010 });
  });
});
