// C1-W — pending withdrawals: a read that failed (or has not finished) shows "—" and the reason,
// never "No pending withdrawals". The Rust side pages eth_getLogs in 1000-block windows; a failed
// window is an error, and the first scan of a long chain answers "still reading chain history"
// until it reaches the head, so the store reads again until it does.
import { describe, it, expect, vi, afterEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

import { Wallet } from "./Wallet";
import { freshState, type AppState } from "../shell/state";
import { store, type Store } from "../shell/store";
import { bridge } from "../bridge";

const stubStore = {
  identity: () => ({ name: "Member", initials: "M", email: "m@example.com", sub: "sub-1", wallet: "0x" + "0".repeat(40), tier: "pilot", role: "member", org: null, real: true }),
} as unknown as Store;

function staking(over: Partial<AppState> = {}): AppState {
  return { ...freshState("p1"), wTab: "staking", signedIn: true, ...over };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("C1-W — pending withdrawals are never an empty list on a failed read", () => {
  it("before the first read, the queue is '—', not 'No pending withdrawals'", () => {
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={staking()} />);
    expect(html).not.toContain("No pending withdrawals");
    expect(html).toContain("—");
  });

  it("a failed window shows '—' and the reason", async () => {
    vi.spyOn(bridge.wallet, "pendingWithdrawals").mockRejectedValue("staking: rpc error: eth_getLogs blocks 8427-9426: rpc transport error: timed out");
    store.setState({ pendingWithdrawals: [], pendingWithdrawalsError: null });
    await store.refreshPendingWithdrawals();
    expect(store.state.pendingWithdrawalsError).toContain("eth_getLogs blocks 8427-9426");
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={staking({ pendingWithdrawalsError: store.state.pendingWithdrawalsError })} />);
    expect(html).toContain("eth_getLogs blocks 8427-9426");
    expect(html).not.toContain("No pending withdrawals");
  });

  it("a successful read clears the reason, and an empty queue then says so", async () => {
    vi.spyOn(bridge.wallet, "pendingWithdrawals").mockResolvedValue([]);
    store.setState({ pendingWithdrawalsError: "earlier failure" });
    await store.refreshPendingWithdrawals();
    expect(store.state.pendingWithdrawalsError).toBeNull();
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={staking({ pendingWithdrawals: [], pendingWithdrawalsError: null })} />);
    expect(html).toContain("No pending withdrawals");
  });

  it("an unfinished first scan reads again until it reaches the head", async () => {
    vi.useFakeTimers();
    const read = vi
      .spyOn(bridge.wallet, "pendingWithdrawals")
      .mockRejectedValueOnce("still reading chain history: blocks 427-200000 of 524862 scanned")
      .mockResolvedValueOnce([{ id: "7", saltWei: "1000000000000000000", requestBlock: 5000, claimableAtBlock: 55400, claimable: true }]);
    await store.refreshPendingWithdrawals();
    expect(store.state.pendingWithdrawalsError).toContain("still reading chain history");
    await vi.advanceTimersByTimeAsync(5_000);
    expect(read).toHaveBeenCalledTimes(2);
    expect(store.state.pendingWithdrawalsError).toBeNull();
    expect(store.state.pendingWithdrawals.map((w) => w.id)).toEqual(["7"]);
  });
});
