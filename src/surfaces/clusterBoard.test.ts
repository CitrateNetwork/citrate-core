// 0.5.4 FL design, board 04: the cluster board's pure models. The 0.6.0 states (a live round's
// requirements) are fixtures here only; the app passes no requirements while Group rounds are not live.
import { describe, expect, it } from "vitest";
import { GIB, NO_ROUNDS_REASON, noRequirementsReason, acceleratorBytes, capacityModel, eligibilityModel, hardwareLine, isGroupSeed, joinOutcome, newlyPaired } from "./clusterBoard";
import type { HardwareFacts } from "../bridge/domains";
import type { FleetDevice, JoinResult } from "../bridge/tauri/fleet";

const gpu24: HardwareFacts = { os: "linux", arch: "x86_64", totalRamBytes: 128 * GIB, unifiedMemory: false, gpuVramBytes: 24 * GIB, diskFreeBytes: 500 * GIB };
const m16: HardwareFacts = { os: "macos", arch: "aarch64", totalRamBytes: 16 * GIB, unifiedMemory: true, gpuVramBytes: null, diskFreeBytes: 100 * GIB };
const noGpu: HardwareFacts = { os: "linux", arch: "x86_64", totalRamBytes: 8 * GIB, unifiedMemory: false, gpuVramBytes: null, diskFreeBytes: null };

const dev = (id: string, label: string, tier: FleetDevice["tier"] = "T2", code?: string): FleetDevice => ({ id, label, tier, role: "heavy", addr: null, pairedAt: 1, via: "issued", ...(code ? { code } : {}) });

describe("hardware receipts", () => {
  it("reads GPU memory, else unified memory, else unknown", () => {
    expect(acceleratorBytes(gpu24)).toBe(24 * GIB);
    expect(acceleratorBytes(m16)).toBe(16 * GIB);
    expect(acceleratorBytes(noGpu)).toBeNull();
    expect(hardwareLine(gpu24)).toBe("GPU, 24 GB memory, 128 GB RAM, 500 GB disk free");
    expect(hardwareLine(m16)).toBe("Apple silicon, 16 GB unified memory, 100 GB disk free");
  });
});

describe("eligibilityModel", () => {
  it("Today: Not known, because Group rounds are not live, and lists what this machine reported", () => {
    const e = eligibilityModel({ kind: "ok", facts: gpu24 }, null);
    expect(e.kind).toBe("unknown");
    expect(e.word).toBe("Not known");
    expect(e.lines[0]).toBe(NO_ROUNDS_REASON);
    expect(e.lines[1]).toContain("24 GB memory");
    expect(e.reprobe).toBe(false);
  });

  it("probe failed: names the failure and offers the probe again, never zero", () => {
    const e = eligibilityModel({ kind: "failed", reason: "driver did not answer in 10 s" }, null);
    expect(e.kind).toBe("unknown");
    expect(e.lines[0]).toBe("The hardware probe failed: driver did not answer in 10 s");
    expect(e.lines[1]).toContain("not shown as zero");
    expect(e.reprobe).toBe(true);
  });

  it("fixture requirements met: Eligible with one line per requirement", () => {
    const e = eligibilityModel({ kind: "ok", facts: gpu24 }, { round: "round 15", minAcceleratorBytes: 16 * GIB, minRamBytes: 32 * GIB });
    expect(e.kind).toBe("eligible");
    expect(e.lines).toEqual(["24 GB GPU memory meets the 16 GB minimum of round 15", "128 GB RAM meets the 32 GB minimum of round 15"]);
  });

  it("fixture requirements missed: Not eligible with a why-not per miss", () => {
    const e = eligibilityModel({ kind: "ok", facts: m16 }, { round: "round 16", minAcceleratorBytes: 24 * GIB, minDiskFreeBytes: 50 * GIB });
    expect(e.kind).toBe("not-eligible");
    expect(e.word).toBe("Not eligible right now");
    expect(e.lines).toContain("needs 24 GB GPU memory for round 16 (this machine has 16 GB)");
    expect(e.lines).toContain("100 GB free disk meets the 50 GB minimum of round 16");
  });

  it("fixture requirement this machine did not report: Not known, not a miss", () => {
    const e = eligibilityModel({ kind: "ok", facts: noGpu }, { round: "round 15", minAcceleratorBytes: 16 * GIB });
    expect(e.kind).toBe("unknown");
    expect(e.lines[0]).toBe("GPU memory not reported, so the 16 GB minimum of round 15 can't be checked");
  });

  it("the Today reason uses the required wording and follows the network's phase", () => {
    expect(NO_ROUNDS_REASON.startsWith("Group rounds not live yet")).toBe(true);
    expect(noRequirementsReason({ phase: "today" })).toBe(NO_ROUNDS_REASON);
    expect(noRequirementsReason({ phase: "unknown", reason: "WorkMarket: rpc down" })).toContain("unknown (WorkMarket: rpc down)");
    expect(noRequirementsReason({ phase: "reading" })).toContain("Reading whether group rounds are live");
    expect(noRequirementsReason({ phase: "rounds-live" })).toContain("No open round's requirements");
    const e = eligibilityModel({ kind: "ok", facts: gpu24 }, null, { phase: "unknown", reason: "x" });
    expect(e.word).toBe("Not known");
    expect(e.lines[0]).not.toBe(NO_ROUNDS_REASON);
  });

  it("while the probe runs it says so", () => {
    expect(eligibilityModel({ kind: "loading" }, null).kind).toBe("checking");
  });
});

describe("capacityModel", () => {
  it("counts only machines that reported; peers are not reported", () => {
    const c = capacityModel({ kind: "ok", facts: gpu24 }, 4, [dev("a", "Studio"), dev("b", "Laptop", null)]);
    expect(c.reported).toBe(1);
    expect(c.accelerator).toBe("24 GB");
    expect(c.notReported).toBe(4);
    expect(c.paired).toEqual([
      { id: "a", label: "Studio", tier: "tier T2" },
      { id: "b", label: "Laptop", tier: "tier not reported" },
    ]);
  });

  it("a failed probe is unknown, not zero memory", () => {
    const c = capacityModel({ kind: "failed", reason: "x" }, 2, []);
    expect(c.reported).toBe(0);
    expect(c.accelerator).toBe("—");
    expect(c.ram).toBe("—");
    expect(c.notReported).toBe(3);
  });
});

describe("joinOutcome", () => {
  const base: JoinResult = { ok: false, device: null, errorKind: null, message: null, tried: [] };
  it("ok names the machine and the code to compare", () => {
    expect(joinOutcome({ ...base, ok: true, device: dev("x", "Studio Mac", "T2", "123456") })).toEqual({ ok: true, text: "Paired with Studio Mac. Both machines show code 123456; check they match." });
  });
  it.each([
    ["link", "not a valid pairing link"],
    ["refused", "refused the pairing"],
    ["unreachable", "Couldn't reach the other machine at any address in the link (2 tried)"],
    ["protocol", "Update Citrate Core on both machines"],
  ] as const)("%s says what happened", (kind, words) => {
    const o = joinOutcome({ ...base, errorKind: kind, tried: ["a", "b"] });
    expect(o.ok).toBe(false);
    expect(o.text).toContain(words);
    expect(o.text).not.toMatch(/!|—/);
  });
});

describe("links", () => {
  it("tells a group link from a pairing link", () => {
    expect(isGroupSeed("citrate-cluster://seed?v=1&g=x")).toBe(true);
    expect(isGroupSeed("citrate://pair?c=1")).toBe(false);
  });
  it("finds the machine that joined since the sheet opened", () => {
    expect(newlyPaired(new Set(["a"]), [dev("a", "A"), dev("b", "B")]).map((d) => d.id)).toEqual(["b"]);
  });
});
