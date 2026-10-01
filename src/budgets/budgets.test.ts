// HUP-S2.3 — Settings → Budgets: pure helpers and the bridge seam.
import { describe, it, expect, vi, beforeEach } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
  isTauri: () => false,
}));

import {
  formatCountdown,
  validateGrantForm,
  statusLabel,
  recordLabel,
  tauriBudgets,
} from "./budgets";

const CEIL = { maxCount: 50, ttlDays: 30 };

describe("Feature: a budget countdown the member can read", () => {
  it("formats days, hours, minutes and seconds, and says expired at zero", () => {
    expect(formatCountdown(0)).toBe("expired");
    expect(formatCountdown(-5)).toBe("expired");
    expect(formatCountdown(59_000)).toBe("59s");
    expect(formatCountdown(3 * 60_000 + 5_000)).toBe("3m 05s");
    expect(formatCountdown(4 * 3_600_000 + 12 * 60_000)).toBe("4h 12m");
    expect(formatCountdown(2 * 86_400_000 + 3 * 3_600_000)).toBe("2d 3h");
  });
});

describe("Feature: the grant form refuses what core would refuse", () => {
  it("accepts a plain https origin inside the ceilings", () => {
    expect(
      validateGrantForm("https://app.example.org", 10, 7, CEIL),
    ).toBeNull();
  });
  it("refuses http, IPs, localhost, paths and out-of-range numbers", () => {
    expect(validateGrantForm("http://app.example.org", 10, 7, CEIL)).toMatch(
      /https/,
    );
    expect(validateGrantForm("https://127.0.0.1", 10, 7, CEIL)).toMatch(/IP/);
    expect(validateGrantForm("https://localhost", 10, 7, CEIL)).toMatch(
      /Local/,
    );
    expect(
      validateGrantForm("https://app.example.org/login", 10, 7, CEIL),
    ).toMatch(/origin only/);
    expect(validateGrantForm("https://app.example.org", 0, 7, CEIL)).toMatch(
      /1 to 50/,
    );
    expect(validateGrantForm("https://app.example.org", 51, 7, CEIL)).toMatch(
      /1 to 50/,
    );
    expect(validateGrantForm("https://app.example.org", 5, 31, CEIL)).toMatch(
      /1 to 30/,
    );
    expect(validateGrantForm("not a url", 5, 7, CEIL)).toMatch(/valid/);
  });
});

describe("Feature: plain-language labels", () => {
  it("labels every budget status and record kind", () => {
    for (const s of [
      "active",
      "revoked",
      "expired",
      "used_up",
      "wallet_changed",
    ]) {
      expect(statusLabel(s)).not.toBe(s);
    }
    for (const k of [
      "budget_granted",
      "budget_revoked",
      "all_budgets_revoked",
      "store_reset",
      "auto_sign",
    ] as const) {
      expect(recordLabel(k).length).toBeGreaterThan(3);
    }
  });
});

describe("Feature: the bridge calls the core commands", () => {
  beforeEach(() => invokeMock.mockReset());
  it("status / grant / revoke / revokeAll / reset map to the web_budget_* commands", async () => {
    invokeMock.mockResolvedValue(null);
    await tauriBudgets.status();
    expect(invokeMock).toHaveBeenLastCalledWith("web_budget_status");
    await tauriBudgets.grant("https://app.example.org", 10, 7);
    expect(invokeMock).toHaveBeenLastCalledWith("web_budget_grant", {
      origin: "https://app.example.org",
      maxCount: 10,
      ttlDays: 7,
    });
    await tauriBudgets.revoke(3);
    expect(invokeMock).toHaveBeenLastCalledWith("web_budget_revoke", { id: 3 });
    await tauriBudgets.revokeAll();
    expect(invokeMock).toHaveBeenLastCalledWith("web_budget_revoke_all");
    await tauriBudgets.reset();
    expect(invokeMock).toHaveBeenLastCalledWith("web_budget_reset");
  });
});

describe("Rule 3: the agent cannot create or change a budget", () => {
  it("no agent tool surface names a web_budget command", async () => {
    const fs = await import("node:fs");
    const path = await import("node:path");
    const dir = path.resolve(__dirname, "../agent");
    const files = fs
      .readdirSync(dir)
      .filter((f) => /\.(ts|tsx|json)$/.test(f) && !/\.test\./.test(f));
    for (const f of files) {
      const src = fs.readFileSync(path.join(dir, f), "utf8");
      expect(src.includes("web_budget_"), f).toBe(false);
    }
  });
});
