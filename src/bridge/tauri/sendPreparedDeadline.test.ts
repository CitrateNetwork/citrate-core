// Join-8 — sign_send_prepared polls for the receipt up to 60 s Rust-side (30 × 2 s), exactly like
// sign_and_broadcast. The webview deadline must sit above that bound, or a real send is reported
// as a timeout while the receipt poll is still running.
import { describe, it, expect } from "vitest";
import { deadlineFor } from "./invoke";

describe("sign_send_prepared deadline", () => {
  it("is above the 60 s Rust-side receipt poll, like sign_and_broadcast", () => {
    expect(deadlineFor("sign_send_prepared")).toBeGreaterThan(60_000);
    expect(deadlineFor("sign_send_prepared")).toBe(deadlineFor("sign_and_broadcast"));
  });
  it("sign_prepare (no send, no poll) keeps a bounded deadline", () => {
    expect(Number.isFinite(deadlineFor("sign_prepare"))).toBe(true);
  });
});
