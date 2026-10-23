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
    // Codex round 5 (P1): approving a claim now closes and revokes before it answers, like revoke.
    // The webview must outwait both, or a slow success reads as a failed approval.
    expect(deadlineFor("group_invite_verify_consume")).toBeGreaterThan(26_000);
  });
});
