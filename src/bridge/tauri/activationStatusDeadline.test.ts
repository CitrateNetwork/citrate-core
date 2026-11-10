// Join-8 follow-up — validator_activation_status is ~18 sequential RPC reads (6 s timeout each),
// bounded Rust-side by an aggregate READ_BUDGET (30 s) plus at most one RPC in flight (6 s).
// The webview deadline must sit above that bound, or a slow but healthy read is thrown away by
// the 12 s default and every activation click fails.
import { describe, it, expect, vi, afterEach } from "vitest";

const rawInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => rawInvoke(...a) }));

import { deadlineFor, invoke } from "./invoke";

const RUST_READ_BUDGET_MS = 30_000;
const RPC_TIMEOUT_MS = 6_000;

afterEach(() => {
  vi.useRealTimers();
  rawInvoke.mockReset();
});

describe("validator_activation_status deadline", () => {
  it("is above the Rust-side read budget plus one RPC timeout", () => {
    expect(deadlineFor("validator_activation_status")).toBeGreaterThan(RUST_READ_BUDGET_MS + RPC_TIMEOUT_MS);
    expect(Number.isFinite(deadlineFor("validator_activation_status"))).toBe(true);
  });

  it("a successful read that takes 20 s reaches the caller through the real invoke wrapper", async () => {
    vi.useFakeTimers();
    const status = { bondActivated: false };
    rawInvoke.mockImplementationOnce(() => new Promise((res) => setTimeout(() => res(status), 20_000)));
    const p = invoke("validator_activation_status", { memberAddress: "0x1", txHash: null });
    const settled = p.then(
      (v) => ({ ok: true as const, v }),
      (e: unknown) => ({ ok: false as const, e }),
    );
    await vi.advanceTimersByTimeAsync(20_000);
    const out = await settled;
    expect(out).toEqual({ ok: true, v: status });
  });
});
