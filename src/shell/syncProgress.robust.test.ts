// #566 review P2s — sync progress on slow or flaky RPCs, a clock that goes back, each sample gate,
// and the contract's words and timing (UI truth contract §1: "Sync stalled" at 60 s; a failed
// public head read is unknown at once, like Node › Status).
import { describe, it, expect } from "vitest";
import {
  HEAD_READ_MS,
  HEAD_READ_TIMEOUT_MS,
  MIN_HEAD_POINTS,
  MIN_LOCAL_POINTS,
  MIN_SPAN_MS,
  STALL_MS,
  SyncTracker,
  emptySeries,
  observe,
  syncLine,
  syncProgress,
  type SyncSeries,
} from "./syncProgress";

const T0 = 1_800_000_000_000;
const flush = async () => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

/** A head reader whose answers the test resolves by hand, in any order. */
function manualHead() {
  const pending: { resolve: (v: number | null) => void; reject: (e: unknown) => void }[] = [];
  const read = () => new Promise<number | null>((resolve, reject) => pending.push({ resolve, reject }));
  return { read, pending };
}

/** Local every 2 s from `l0` at `lr`/s; head readings every `headEvery` s from `h0` at `hr`/s. */
function series(o: { seconds: number; l0: number; lr: number; h0: number; hr: number; headEvery?: number; localEvery?: number; headFrom?: number; localFrom?: number }): { s: SyncSeries; now: number } {
  let s = emptySeries();
  const he = o.headEvery ?? 5;
  const le = o.localEvery ?? 2;
  for (let sec = 0; sec <= o.seconds; sec++) {
    const t = T0 + sec * 1000;
    if (sec >= (o.localFrom ?? 0) && sec % le === 0) s = observe(s, "local", t, Math.round(o.l0 + o.lr * sec));
    if (sec >= (o.headFrom ?? 0) && sec % he === 0) s = observe(s, "head", t, Math.round(o.h0 + o.hr * sec));
  }
  return { s, now: T0 + o.seconds * 1000 };
}

describe("P2.1 slow or flaky RPCs", () => {
  it("does not start a head read while one is in flight", async () => {
    let now = T0;
    const tr = new SyncTracker(() => now);
    const h = manualHead();
    let calls = 0;
    const read = () => {
      calls++;
      return h.read();
    };
    // Every fold is past the throttle and still inside HEAD_READ_TIMEOUT_MS of the first read.
    for (let i = 0; i < 3; i++) {
      tr.fold(100_000 + i * 80, read);
      now += HEAD_READ_MS;
    }
    expect(now - T0).toBeLessThan(HEAD_READ_TIMEOUT_MS);
    expect(calls).toBe(1);
    h.pending[0].resolve(200_000);
    await flush();
    tr.fold(100_600, read);
    expect(calls).toBe(2);
  });

  it("a read that hangs past HEAD_READ_TIMEOUT_MS is unknown at once, a new read starts, and the late answer is ignored", async () => {
    let now = T0;
    const tr = new SyncTracker(() => now);
    const h = manualHead();
    tr.fold(100_000, h.read); // read #0 hangs
    now += HEAD_READ_TIMEOUT_MS;
    const p = tr.fold(100_100, h.read); // timed out: unknown, and read #1 starts
    expect(p).toEqual({ kind: "unknown", missing: "head" });
    expect(h.pending).toHaveLength(2);
    h.pending[1].resolve(150_000);
    await flush();
    now += 1000;
    // The stale, out-of-order answer, far outside the noise band: without the generation check it
    // would restart the head series at 120,000.
    h.pending[0].resolve(120_000);
    await flush();
    now += 2000;
    const q = tr.fold(100_200, h.read);
    expect(q.kind).toBe("syncing");
    if (q.kind === "syncing") expect(q.head).toBe(150_000);
  });

  it("small drops (a load-balanced backend a few blocks behind) keep the series and the ETA", () => {
    // The head alternates between two backends 4 blocks apart; the node catches up at ~40/s.
    let s = emptySeries();
    for (let sec = 0; sec <= 120; sec++) {
      const t = T0 + sec * 1000;
      if (sec % 2 === 0) s = observe(s, "local", t, 100_000 + 40 * sec);
      if (sec % 5 === 0) s = observe(s, "head", t, 130_000 + Math.round(0.5 * sec) - ((sec / 5) % 2 === 1 ? 4 : 0));
    }
    expect(s.head.length).toBeGreaterThanOrEqual(20);
    const p = syncProgress(s, T0 + 120_000);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("catching-up");
    expect(p.rate).toBeGreaterThan(38);
    expect(p.rate).toBeLessThan(41);
  });

  it("this node's height is never held at an earlier max: a 4,000-block rollback reads behind, not Synced or stalled (#569 review P1)", () => {
    // A node at 500,000, within 3 blocks of the head, rolls back 4,000 blocks and then re-applies.
    let s = emptySeries();
    for (let sec = 0; sec <= 60; sec += 2) {
      s = observe(s, "local", T0 + sec * 1000, 499_970 + sec / 2);
      if (sec % 6 === 0) s = observe(s, "head", T0 + sec * 1000, 499_973 + sec / 2);
    }
    expect(syncProgress(s, T0 + 60_000).kind).toBe("synced");
    s = observe(s, "local", T0 + 62_000, 496_000);
    let p = syncProgress(s, T0 + 62_000);
    expect(p.kind).toBe("syncing");
    if (p.kind === "syncing") {
      expect(p.local).toBe(496_000);
      expect(p.behind).toBe(p.head - 496_000);
    }
    // It re-applies to 497,170 over the next 70 s: real height, real behind, not "Sync stalled".
    let t = T0 + 62_000;
    for (let i = 1; i <= 35; i++) {
      t += 2000;
      s = observe(s, "local", t, 496_000 + Math.round((1_170 * i) / 35));
      if (i % 3 === 0) s = observe(s, "head", t, 500_003 + i);
    }
    p = syncProgress(s, t);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.local).toBe(497_170);
    expect(p.behind).toBe(p.head - 497_170);
    expect(p.behind).toBeGreaterThan(2_800);
    expect(p.phase).not.toBe("stalled");
    expect(syncLine(p)).not.toMatch(/^Synced|Sync stalled/);
  });

  it("a small local drop restarts the local series (only the public head gets the noise rule)", () => {
    let s = emptySeries();
    s = observe(s, "local", T0, 100_000);
    s = observe(s, "local", T0 + 2000, 100_080);
    s = observe(s, "local", T0 + 4000, 100_075);
    expect(s.local).toEqual([{ t: T0 + 4000, v: 100_075 }]);
  });

  it("a drop of up to 8 blocks is small even at a low height; one of 9 restarts there", () => {
    let s = observe(observe(emptySeries(), "head", T0, 200), "head", T0 + 1, 192);
    expect(s.head).toHaveLength(2);
    s = observe(observe(emptySeries(), "head", T0, 200), "head", T0 + 1, 191);
    expect(s.head).toEqual([{ t: T0 + 1, v: 191 }]);
  });

  it("a large drop (beyond 1% of the height) still restarts the series", () => {
    let s = emptySeries();
    s = observe(s, "head", T0, 500_000);
    s = observe(s, "head", T0 + 1, 500_000 - 4_999); // within 1%: kept at the max
    expect(s.head).toHaveLength(2);
    s = observe(s, "head", T0 + 2, 500_000 - 5_001);
    expect(s.head).toEqual([{ t: T0 + 2, v: 494_999 }]);
  });

  it("a failed head read is unknown at once, not after 60 s (like Node › Status)", async () => {
    let now = T0;
    const tr = new SyncTracker(() => now);
    let fail = false;
    let head = 200_000;
    const read = async () => {
      if (fail) throw new Error("rpc 503");
      return (head += 3);
    };
    for (let i = 0; i < 30; i++) {
      tr.fold(100_000 + i * 80, read);
      await flush();
      now += 2000;
    }
    expect(tr.fold(102_400, read).kind).toBe("syncing");
    await flush();
    fail = true;
    now += HEAD_READ_MS;
    tr.fold(102_480, read); // starts the failing read
    await flush();
    now += 2000;
    expect(tr.fold(102_560, read)).toEqual({ kind: "unknown", missing: "head" });
    fail = false;
    now += HEAD_READ_MS;
    tr.fold(102_640, read);
    await flush();
    now += 2000;
    expect(tr.fold(102_720, read).kind).toBe("syncing");
  });

  it("a head read of 0x0 is a failed read: unknown at once", () => {
    let s = series({ seconds: 60, l0: 1_000, lr: 40, h0: 9_000, hr: 0.5 }).s;
    s = observe(s, "head", T0 + 61_000, null);
    expect(syncProgress(s, T0 + 61_000)).toEqual({ kind: "unknown", missing: "head" });
  });
});

describe("P2.2 the clock going back", () => {
  it("wall time set back an hour restarts the series instead of freezing a stale line", () => {
    const { s: s0 } = series({ seconds: 120, l0: 100_000, lr: 40, h0: 130_000, hr: 0.5 });
    const back = T0 + 120_000 - 3_600_000;
    let s = s0;
    for (let sec = 2; sec <= 600; sec += 2) {
      s = observe(s, "local", back + sec * 1000, 104_800); // no progress after the jump
      if (sec % 6 === 0) s = observe(s, "head", back + sec * 1000, 130_060 + Math.round(sec / 2));
    }
    const p = syncProgress(s, back + 600_000);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    expect(p.phase).toBe("stalled");
    expect(syncLine(p)).not.toMatch(/catching up|about/);
  });

  it("a reading from the future (now before the last reading) is unknown, not fresh", () => {
    const { s } = series({ seconds: 60, l0: 1_000, lr: 40, h0: 9_000, hr: 0.5 });
    expect(syncProgress(s, T0 - 3_600_000)).toEqual({ kind: "unknown", missing: "local" });
  });

  it("the tracker's clock is monotonic: it does not read Date.now", () => {
    let mono = 0;
    const tr = new SyncTracker(() => mono);
    const realNow = Date.now;
    Date.now = () => {
      throw new Error("the window must not read wall time");
    };
    try {
      tr.fold(100, async () => 200);
      mono += 2000;
      expect(() => tr.fold(180, async () => 200)).not.toThrow();
    } finally {
      Date.now = realNow;
    }
  });
});

describe("P2.3 each sample gate holds on its own", () => {
  const ok = (s: SyncSeries, now: number) => {
    const p = syncProgress(s, now);
    if (p.kind !== "syncing") throw new Error("expected syncing, got " + p.kind);
    return p;
  };

  it("the control: every gate met gives a rate", () => {
    const { s, now } = series({ seconds: 60, l0: 1_000, lr: 40, h0: 9_000, hr: 0.5 });
    expect(ok(s, now).phase).toBe("catching-up");
  });

  it("local span < MIN_SPAN_MS: estimating (many local points, long head)", () => {
    // Local only in the last 20 s (11 points), head over 60 s.
    const { s, now } = series({ seconds: 60, l0: 1_000, lr: 40, h0: 9_000, hr: 0.5, localEvery: 2, localFrom: 40 });
    expect(s.local.length).toBeGreaterThanOrEqual(MIN_LOCAL_POINTS);
    expect(s.local[s.local.length - 1].t - s.local[0].t).toBeLessThan(MIN_SPAN_MS);
    expect(ok(s, now).phase).toBe("estimating");
  });

  it("head span < MIN_SPAN_MS: estimating (3+ head points within 10 s, long local)", () => {
    const { s, now } = series({ seconds: 60, l0: 1_000, lr: 40, h0: 9_000, hr: 0.5, headEvery: 5, headFrom: 50 });
    expect(s.head.length).toBeGreaterThanOrEqual(MIN_HEAD_POINTS);
    expect(s.head[s.head.length - 1].t - s.head[0].t).toBeLessThan(MIN_SPAN_MS);
    expect(ok(s, now).phase).toBe("estimating");
  });

  it("fewer than MIN_LOCAL_POINTS local readings: estimating (long span)", () => {
    const { s, now } = series({ seconds: 60, l0: 1_000, lr: 40, h0: 9_000, hr: 0.5, localEvery: 20 });
    expect(s.local.length).toBe(MIN_LOCAL_POINTS - 1);
    expect(ok(s, now).phase).toBe("estimating");
  });

  it("fewer than MIN_HEAD_POINTS head readings: estimating (long span)", () => {
    const { s, now } = series({ seconds: 60, l0: 1_000, lr: 40, h0: 9_000, hr: 0.5, headEvery: 60 });
    expect(s.head.length).toBe(MIN_HEAD_POINTS - 1);
    expect(ok(s, now).phase).toBe("estimating");
  });
});

describe("P2.4 contract words and timing", () => {
  it("No peers (contract §1): net_peerCount 0 for 60 s while behind reads \"No peers\", not Sync stalled", () => {
    const { s, now } = series({ seconds: 60, l0: 5_000, lr: 0, h0: 9_000, hr: 0.5 });
    expect(syncLine(syncProgress(s, now, { noPeersForMs: 60_000 }))).toBe("No peers · 4,030 blocks behind");
    expect(syncLine(syncProgress(s, now, { noPeersForMs: 58_000 }))).toBe("Sync stalled · 4,030 blocks behind");
    const p = syncProgress(s, now, { noPeersForMs: 60_000 });
    expect(p.kind === "syncing" && p.rate === null && p.etaSec === null).toBe(true);
  });

  it("the tracker times zero peers on its clock and clears on the first peer", async () => {
    let now = T0;
    const tr = new SyncTracker(() => now);
    let head = 9_000;
    const read = async () => (head += 2);
    let p = tr.fold(5_000, read, 0);
    for (let i = 0; i < 30; i++) {
      await flush();
      now += 2000;
      p = tr.fold(5_000, read, 0);
    }
    expect(syncLine(p)).toMatch(/^No peers · /);
    await flush();
    now += 2000;
    expect(syncLine(tr.fold(5_000, read, 1))).toMatch(/^Sync stalled · /);
  });

  it("stalls at 60 s with the contract word, and no implied action", () => {
    expect(STALL_MS).toBe(60_000);
    const { s, now } = series({ seconds: 60, l0: 5_000, lr: 0, h0: 9_000, hr: 0.5 });
    const p = syncProgress(s, now);
    expect(syncLine(p)).toBe("Sync stalled · 4,030 blocks behind");
    expect(syncLine(p)).not.toMatch(/checking|peers/);
  });
});
