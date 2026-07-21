// Q-E.2 (C-5) — failed-tx display. The activity read passes the REAL receipt
// `status` through the bridge (1 ok / 0 reverted / null pending); the Activity
// table must MARK a failed tx so it can no longer render identically to a
// success (the bug this closes). We render the Activity sub-tab and assert:
//   - a status=0 row shows a "failed" indicator (positive);
//   - a status=1 row shows NO failed indicator (negative control — a success is
//     unmarked, so the "failed" badge is genuinely status-driven, not constant);
//   - a status=null row shows a "pending" indicator.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Wallet } from "./Wallet";
import { freshState, type AppState, type Activity } from "../shell/state";
import type { Store } from "../shell/store";

// Wallet calls store.identity() at render and refreshes in a useEffect (never
// fired by a static server render). A tiny stub covers the render-time call.
const stubStore = {
  identity: () => ({
    name: "Member",
    initials: "M",
    email: "member@example.com",
    sub: "sub-1",
    wallet: "0x" + "0".repeat(40),
    tier: "pilot",
    role: "member",
    org: null,
    real: true,
  }),
} as unknown as Store;

function activityState(rows: Activity[]): AppState {
  const s = freshState("p1");
  s.wTab = "activity";
  s.activity = rows;
  return s;
}

const row = (over: Partial<Activity>): Activity => ({ id: "a1", kind: "Send", amount: "−40.00 SALT", hash: "0x" + "ab".repeat(32), ts: Date.now(), ...over });

describe("Wallet activity — Q-E.2 failed/pending status marker", () => {
  it("a reverted tx (status=0) renders a failed indicator", () => {
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={activityState([row({ status: 0 })])} />);
    expect(html).toContain('data-tx-status="failed"');
    expect(html).toContain("failed");
  });

  it("a confirmed tx (status=1) renders NO failed indicator (negative control)", () => {
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={activityState([row({ status: 1 })])} />);
    expect(html).not.toContain('data-tx-status="failed"');
    expect(html).not.toContain('data-tx-status="pending"');
  });

  it("an unconfirmed tx (status=null) renders a pending indicator", () => {
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={activityState([row({ status: null })])} />);
    expect(html).toContain('data-tx-status="pending"');
    expect(html).not.toContain('data-tx-status="failed"');
  });
});
