// HUP-S6.5: the in-app faucet: pure helpers and the bridge seam.
import { describe, it, expect, vi, beforeEach } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
  isTauri: () => false,
}));

import {
  faucetApi,
  formatSalt,
  healthLine,
  nextEligible,
  outcomeLabel,
  shortAddress,
  tauriFaucet,
  type FaucetStatus,
} from "./faucet";

function st(patch: Partial<FaucetStatus> = {}): FaucetStatus {
  return {
    enabled: false,
    budget: null,
    wallet: "0x00000000000000000000000000000000000000aa",
    walletError: null,
    walletMatches: false,
    storeError: null,
    health: { reachable: true, ready: true, detail: "The faucet is up and can send a drip." },
    nextEligibleAtMs: null,
    faucetNextEligibleAtMs: null,
    faucetEligibilityKnown: true,
    ledger: [],
    faucetUrl: "https://faucet.citrate.ai",
    faucetPage: "https://faucet.citrate.ai/?address=0x00000000000000000000000000000000000000aa",
    deployGasLimit: 2_000_000,
    dripWei: "10000000000000000000",
    windowHours: 24,
    maxPerWindow: 1,
    pendingOwnerSignOff: ["O-1 ... Pending owner sign-off."],
    nowMs: 1_000,
    ...patch,
  };
}

describe("Feature: amounts and times the member can read", () => {
  it("formats wei as SALT without float rounding", () => {
    expect(formatSalt("10000000000000000000", 2)).toBe("10 SALT");
    expect(formatSalt("2000000000000000")).toBe("0.002 SALT");
    expect(formatSalt("1585694000000000")).toBe("0.001585 SALT");
    expect(formatSalt("0")).toBe("0 SALT");
    expect(formatSalt(null)).toBe("unknown");
    expect(formatSalt("-1")).toBe("unknown");
  });
  it("labels every outcome and shortens addresses", () => {
    for (const o of ["sent", "rate_limited", "challenge_required", "refused", "unreachable", "unknown"] as const) {
      expect(outcomeLabel(o).length).toBeGreaterThan(0);
    }
    expect(shortAddress("0x00000000000000000000000000000000000000aa")).toBe("0x0000…00aa");
    expect(shortAddress(null)).toBe("no wallet");
  });
  it("next eligible is the later of the app's window and the faucet's answer, if still ahead", () => {
    expect(nextEligible(st())).toBeNull();
    expect(nextEligible(st({ nextEligibleAtMs: 5_000 }))).toBe(5_000);
    expect(nextEligible(st({ nextEligibleAtMs: 5_000, faucetNextEligibleAtMs: 9_000 }))).toBe(9_000);
    expect(nextEligible(st({ nextEligibleAtMs: 500 }))).toBeNull();
  });
  it("an unreachable faucet is said plainly", () => {
    expect(healthLine({ reachable: false, ready: null, detail: "connection refused" })).toMatch(/^Faucet unreachable/);
  });
});

describe("Feature: the bridge maps to core's faucet commands", () => {
  beforeEach(() => invokeMock.mockReset());
  it("status / grant / revoke / request / openChallenge", async () => {
    invokeMock.mockResolvedValue(null);
    await tauriFaucet.status();
    expect(invokeMock).toHaveBeenLastCalledWith("faucet_status");
    await tauriFaucet.grant();
    expect(invokeMock).toHaveBeenLastCalledWith("faucet_grant");
    await tauriFaucet.revoke();
    expect(invokeMock).toHaveBeenLastCalledWith("faucet_revoke");
    await tauriFaucet.request("0x" + "ab".repeat(32));
    expect(invokeMock).toHaveBeenLastCalledWith("faucet_request", { initcodeHash: "0x" + "ab".repeat(32) });
    await tauriFaucet.openChallenge();
    expect(invokeMock).toHaveBeenLastCalledWith("faucet_open_challenge");
  });
  it("is unavailable outside the desktop app", () => {
    expect(faucetApi()).toBeNull();
  });
});

describe("Rule 3 / HIC: the agent cannot turn the faucet on", () => {
  it("no agent tool surface names faucet_grant or faucet_revoke", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const dir = path.resolve(__dirname, "../agent");
    const files = fs
      .readdirSync(dir)
      .filter((f) => /\.(ts|tsx|json)$/.test(f) && !/\.test\./.test(f));
    for (const f of files) {
      const src = fs.readFileSync(path.join(dir, f), "utf8");
      expect(src.includes("faucet_grant"), f).toBe(false);
      expect(src.includes("faucet_revoke"), f).toBe(false);
    }
  });
});
