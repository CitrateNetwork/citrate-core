// HUP-S2.3 — Settings → Budgets renders budgets, countdowns, records and the honest state.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { BudgetsPanel } from "./BudgetsPanel";
import type { WebBudgetStatus } from "./budgets";

const NOW = 1_790_856_000_000;

function status(patch: Partial<WebBudgetStatus> = {}): WebBudgetStatus {
  return {
    snapshot: {
      health: { state: "ok" },
      budgets: [
        {
          id: 2,
          origin: "https://app.example.org",
          principal: "hermes",
          chainId: 40204,
          maxCount: 10,
          usedCount: 3,
          remaining: 7,
          grantedAtMs: NOW - 1000,
          expiresAtMs: NOW + 2 * 86_400_000 + 3 * 3_600_000,
          revokedAtMs: null,
          status: "active",
        },
        {
          id: 1,
          origin: "https://old.example.org",
          principal: "hermes",
          chainId: 40204,
          maxCount: 5,
          usedCount: 1,
          remaining: 4,
          grantedAtMs: NOW - 5000,
          expiresAtMs: NOW + 1000,
          revokedAtMs: NOW - 10,
          status: "revoked",
        },
      ],
      records: [
        {
          recordId: 4,
          kind: "auto_sign",
          budgetId: 2,
          principal: "hermes",
          origin: "https://app.example.org",
          payloadDigest: "0x" + "ab".repeat(32),
          statement: "Sign in to Example.",
          nonce: "abcdef0123456789AA",
          requestId: null,
          signerAddress: "0x9858effd232b4033e47d90003d41ec34ecaeda94",
          atMs: NOW - 500,
          remainingAfter: 7,
          note: null,
          prevHash: "0x00",
          hash: "0x01",
          status: "signed",
        },
      ],
      headHash: "0x01",
      recordCount: 4,
    },
    attestation: {
      available: false,
      reason:
        "Automatic sign-in needs the managed browser, which is not in this build yet. Until then every sign-in request asks you, even for sites with a budget.",
    },
    defaults: { maxCount: 10, ttlDays: 7, pendingOwnerSignoff: true },
    ceilings: { maxCount: 50, ttlDays: 30 },
    rate: { minGapSeconds: 30, windowMax: 20, windowHours: 24 },
    walletAddress: "0x9858effd232b4033e47d90003d41ec34ecaeda94",
    nowMs: NOW,
    ...patch,
  };
}

describe("Feature: Settings → Budgets", () => {
  it("lists each budget with its count, countdown and a revoke control", () => {
    const html = renderToStaticMarkup(<BudgetsPanel initial={status()} />);
    expect(html).toContain("https://app.example.org");
    expect(html).toContain("3 of 10 used");
    expect(html).toContain("2d 3h");
    expect(html).toContain("Revoke");
    expect(html).toContain("Revoke all");
    // A revoked budget shows as revoked, with no live countdown.
    expect(html).toContain("Revoked");
  });

  it("is honest that auto sign-in is not active yet and that defaults are placeholders", () => {
    const html = renderToStaticMarkup(<BudgetsPanel initial={status()} />);
    expect(html).toContain("managed browser");
    expect(html).toMatch(/pending owner sign-off/i);
    expect(html).not.toMatch(/HITL|dogfood/i);
    expect(html).not.toContain("—");
  });

  it("shows every signed-for-you record with origin, statement and nonce", () => {
    const html = renderToStaticMarkup(<BudgetsPanel initial={status()} />);
    expect(html).toContain("Signed for you");
    expect(html).toContain("Sign in to Example.");
    expect(html).toContain("abcdef0123456789AA");
  });

  it("with no budgets says every sign-in asks", () => {
    const s = status();
    s.snapshot.budgets = [];
    s.snapshot.records = [];
    const html = renderToStaticMarkup(<BudgetsPanel initial={s} />);
    expect(html).toMatch(/No budgets/);
    expect(html).toMatch(/every sign-in asks you/i);
  });

  it("when the store failed its integrity check, budgets are off and a reset is offered", () => {
    const s = status();
    s.snapshot.health = {
      state: "failed",
      reason: "the budget file failed its integrity check",
    };
    const html = renderToStaticMarkup(<BudgetsPanel initial={s} />);
    expect(html).toContain("the budget file failed its integrity check");
    expect(html).toContain("Reset budgets");
  });

  it("outside the desktop app it says budgets live there", () => {
    const html = renderToStaticMarkup(
      <BudgetsPanel
        initial={null}
        unavailable="Budgets are managed in the Citrate Core desktop app."
      />,
    );
    expect(html).toContain("desktop app");
  });
});
