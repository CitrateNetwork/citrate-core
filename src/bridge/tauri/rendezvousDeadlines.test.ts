// GROW-S2 (Join 9) review — the webview waits longer than the Rust bound for the rendezvous verbs,
// so the Rust error (with its real cause) always arrives first.
import { describe, it, expect } from "vitest";
import { deadlineFor } from "./invoke";

describe("rendezvous invoke deadlines", () => {
  it("cover the 15 s rendezvous IPC bound", () => {
    expect(deadlineFor("rendezvous_tick")).toBeGreaterThan(18_000);
    expect(deadlineFor("rendezvous_cancel")).toBeGreaterThan(36_000);
    expect(deadlineFor("rendezvous_forget")).toBeGreaterThan(36_000);
    expect(deadlineFor("group_invite_create")).toBeGreaterThan(34_000);
    expect(deadlineFor("group_invite_revoke")).toBeGreaterThan(26_000);
    // rendezvousJoin, then (on the §12 fallback only) the legacy redeem.
    expect(deadlineFor("join_redeem")).toBeGreaterThan(26_000);
    expect(deadlineFor("group_invite_redeem")).toBeGreaterThan(26_000);
  });
});
