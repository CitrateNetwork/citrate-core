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
import { store, PENDING_SCAN_RETRY_MS, PENDING_CATCHUP_RETRY_MS, PENDING_INDEXING_RETRY_MS, PENDING_TIMEOUT_RETRY_MS, type Store } from "../shell/store";
import { bridge } from "../bridge";
import { InvokeTimeout } from "../bridge/tauri/invoke";
import { readFileSync } from "node:fs";
import { join } from "node:path";

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
    vi.spyOn(bridge.wallet, "pendingWithdrawals").mockResolvedValue({ withdrawals: [], confirmed: true, complete: true, note: null });
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
      .mockRejectedValueOnce({ kind: "scanning", message: "still reading chain history: the time budget ran out before the node was read" })
      .mockResolvedValueOnce({ withdrawals: [{ id: "7", saltWei: "1000000000000000000", requestBlock: 5000, claimableAtBlock: 55400, claimable: true }], confirmed: true, complete: true, note: null });
    await store.refreshPendingWithdrawals();
    expect(store.state.pendingWithdrawalsError).toContain("still reading chain history");
    await vi.advanceTimersByTimeAsync(5_000);
    expect(read).toHaveBeenCalledTimes(2);
    expect(store.state.pendingWithdrawalsError).toBeNull();
    expect(store.state.pendingWithdrawals.map((w) => w.id)).toEqual(["7"]);
  });

  it("a node still indexing shows '— · pending withdrawals: still indexing chain history' and reads again", async () => {
    vi.useFakeTimers();
    const read = vi
      .spyOn(bridge.wallet, "pendingWithdrawals")
      .mockRejectedValueOnce({ kind: "indexing", message: "still indexing chain history (blocks 427-1426): log index warming, retry" })
      .mockResolvedValueOnce({ withdrawals: [], confirmed: true, complete: true, note: null });
    await store.refreshPendingWithdrawals();
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={staking({ pendingWithdrawals: [], pendingWithdrawalsError: store.state.pendingWithdrawalsError })} />);
    expect(html).toContain("— · pending withdrawals: still indexing chain history");
    expect(html).not.toContain("No pending withdrawals");
    await vi.advanceTimersByTimeAsync(15_000);
    expect(read).toHaveBeenCalledTimes(2);
    expect(store.state.pendingWithdrawalsError).toBeNull();
  });

  it("overlapping refreshes share one read, so retry chains do not multiply", async () => {
    let release!: (v: never) => void;
    const read = vi.spyOn(bridge.wallet, "pendingWithdrawals").mockReturnValue(new Promise((r) => (release = r as never)));
    const a = store.refreshPendingWithdrawals();
    const b = store.refreshPendingWithdrawals();
    expect(read).toHaveBeenCalledTimes(1);
    release({ withdrawals: [], confirmed: true, complete: true, note: null } as never);
    await Promise.all([a, b]);
    expect(store.state.pendingWithdrawalsError).toBeNull();
    // Once settled, a new refresh reads again.
    read.mockResolvedValue({ withdrawals: [], confirmed: true, complete: true, note: null });
    await store.refreshPendingWithdrawals();
    expect(read).toHaveBeenCalledTimes(2);
  });

  // DGX (#371): until citrate_getLogs (chain#334, #336, #337) is on the public RPC, the scan
  // cannot prove it read up to the chain head. An empty list is then "can't confirm", never "No
  // pending withdrawals"; rows that were found (each confirmed from contract state) still show.
  it("an unconfirmed read shows '—' and can't confirm, never 'No pending withdrawals'", async () => {
    vi.spyOn(bridge.wallet, "pendingWithdrawals").mockResolvedValue({ withdrawals: [], confirmed: false, complete: true, note: null });
    await store.refreshPendingWithdrawals();
    expect(store.state.pendingWithdrawalsUnconfirmed).toBe(true);
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={staking({ pendingWithdrawals: [], pendingWithdrawalsError: null, pendingWithdrawalsUnconfirmed: true, complete: true, note: null })} />);
    expect(html).toContain("can&#x27;t confirm");
    expect(html).not.toContain("No pending withdrawals");
  });

  it("an unconfirmed read keeps the rows it found and says it can't confirm the rest", () => {
    const w = { id: "9", saltWei: "2000000000000000000", requestBlock: 5000, claimableAtBlock: 55400, claimable: false };
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={staking({ pendingWithdrawals: [w], pendingWithdrawalsError: null, pendingWithdrawalsUnconfirmed: true, height: 6000 })} />);
    expect(html).toContain("2.00 SALT");
    expect(html).toContain("can&#x27;t confirm");
  });

  // #371 round 5: the Rust read says whether it reached the head ("complete") and, when not
  // confirmed, why ("note"). An incomplete read is read again soon; a complete but unconfirmed one
  // (the node is behind, or gives no evidence) is read again later. The note is shown.
  it("an incomplete read is read again soon, and its found rows are shown", async () => {
    vi.useFakeTimers();
    const w = { id: "1", saltWei: "1000000000000000000", requestBlock: 900, claimableAtBlock: 51300, claimable: false };
    const read = vi
      .spyOn(bridge.wallet, "pendingWithdrawals")
      .mockResolvedValueOnce({ withdrawals: [w], confirmed: false, complete: false, note: "reading chain history: blocks 427-12426 of 30000 read" } as never)
      .mockResolvedValueOnce({ withdrawals: [w], confirmed: true, complete: true, note: null } as never);
    await store.refreshPendingWithdrawals();
    expect(store.state.pendingWithdrawals.map((x) => x.id)).toEqual(["1"]);
    expect(store.state.pendingWithdrawalsNote).toContain("reading chain history");
    await vi.advanceTimersByTimeAsync(PENDING_SCAN_RETRY_MS + 10);
    expect(read).toHaveBeenCalledTimes(2);
    expect(store.state.pendingWithdrawalsUnconfirmed).toBe(false);
  });

  it("a complete but unconfirmed read is read again later and shows why", async () => {
    vi.useFakeTimers();
    const read = vi
      .spyOn(bridge.wallet, "pendingWithdrawals")
      .mockResolvedValue({ withdrawals: [], confirmed: false, complete: true, note: "the node has served blocks up to 3000 of 5000" } as never);
    await store.refreshPendingWithdrawals();
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={staking({ pendingWithdrawals: [], pendingWithdrawalsError: null, pendingWithdrawalsUnconfirmed: true, pendingWithdrawalsNote: store.state.pendingWithdrawalsNote })} />);
    expect(html).toContain("the node has served blocks up to 3000 of 5000");
    expect(html).not.toContain("No pending withdrawals");
    await vi.advanceTimersByTimeAsync(PENDING_SCAN_RETRY_MS + 10);
    expect(read).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(PENDING_CATCHUP_RETRY_MS);
    expect(read).toHaveBeenCalledTimes(2);
  });

  // #371 @66b6fd4: if the command still misses the webview's IPC deadline (InvokeTimeout), the
  // queue shows "—" and the reason, and is read again; the Rust scan kept its progress.
  it("an IPC timeout shows '—' and is read again", async () => {
    vi.useFakeTimers();
    const read = vi
      .spyOn(bridge.wallet, "pendingWithdrawals")
      .mockRejectedValueOnce(new InvokeTimeout("wallet_pending_withdrawals", 30_000))
      .mockResolvedValueOnce({ withdrawals: [], confirmed: true, complete: true, note: null });
    store.setState({ pendingWithdrawals: [], pendingWithdrawalsError: null });
    await store.refreshPendingWithdrawals();
    expect(store.state.pendingWithdrawalsError).toContain("timed out");
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={staking({ pendingWithdrawals: [], pendingWithdrawalsError: store.state.pendingWithdrawalsError })} />);
    expect(html).not.toContain("No pending withdrawals");
    await vi.advanceTimersByTimeAsync(PENDING_TIMEOUT_RETRY_MS + 10);
    expect(read).toHaveBeenCalledTimes(2);
    expect(store.state.pendingWithdrawalsError).toBeNull();
  });

  // #371 DGX review @ff96e43: the retry timer follows the structured error kind from Rust, never
  // the words of the message (which may change, or be translated).
  it("the retry follows the error kind, not the message text", async () => {
    vi.useFakeTimers();
    const read = vi
      .spyOn(bridge.wallet, "pendingWithdrawals")
      .mockRejectedValueOnce({ kind: "scanning", message: "blocks 427-1426 of 5000 read" })
      .mockRejectedValueOnce({ kind: "indexing", message: "blocks 427-1426: warming" })
      .mockRejectedValueOnce({ kind: "failed", message: "still reading chain history" })
      .mockResolvedValue({ withdrawals: [], confirmed: true, complete: true, note: null });
    await store.refreshPendingWithdrawals();
    expect(store.state.pendingWithdrawalsError).toBe("blocks 427-1426 of 5000 read");
    await vi.advanceTimersByTimeAsync(PENDING_SCAN_RETRY_MS + 10);
    expect(read).toHaveBeenCalledTimes(2);
    expect(store.state.pendingWithdrawalsError).toBe("blocks 427-1426: warming");
    await vi.advanceTimersByTimeAsync(PENDING_SCAN_RETRY_MS + 10);
    expect(read).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(PENDING_INDEXING_RETRY_MS);
    expect(read).toHaveBeenCalledTimes(3);
    // `failed`, even with words that look like a budget stop: shown, not retried on a timer.
    expect(store.state.pendingWithdrawalsError).toBe("still reading chain history");
    await vi.advanceTimersByTimeAsync(PENDING_CATCHUP_RETRY_MS + PENDING_TIMEOUT_RETRY_MS);
    expect(read).toHaveBeenCalledTimes(3);
  });

  // #371 Codex @31c065f: the store keeps ONE retry timer. A fresh read (a Wallet visit after the
  // last read finished) replaces the pending timer instead of starting a second chain, and a
  // confirmed read leaves no timer behind.
  it("sequential refreshes keep one retry chain, and a confirmed read ends it", async () => {
    vi.useFakeTimers();
    const unconfirmed = { withdrawals: [], confirmed: false, complete: true, note: "the RPC does not serve citrate_getLogs" };
    const read = vi.spyOn(bridge.wallet, "pendingWithdrawals").mockResolvedValue(unconfirmed as never);
    await store.refreshPendingWithdrawals(); // t=0: a retry at 30 s
    await vi.advanceTimersByTimeAsync(5_000);
    await store.refreshPendingWithdrawals(); // t=5 s (Wallet opened again): the retry moves to 35 s
    expect(read).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(PENDING_CATCHUP_RETRY_MS + 1_000); // t=36 s
    expect(read).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(PENDING_CATCHUP_RETRY_MS); // t=66 s
    expect(read).toHaveBeenCalledTimes(4);
    // A refresh that reads confirmed ends the chain: nothing is read on a timer after it.
    read.mockResolvedValue({ withdrawals: [], confirmed: true, complete: true, note: null } as never);
    await store.refreshPendingWithdrawals();
    expect(read).toHaveBeenCalledTimes(5);
    await vi.advanceTimersByTimeAsync(5 * PENDING_CATCHUP_RETRY_MS);
    expect(read).toHaveBeenCalledTimes(5);
  });

  it("a terminal failure after a scheduled retry leaves no timer behind", async () => {
    vi.useFakeTimers();
    const read = vi
      .spyOn(bridge.wallet, "pendingWithdrawals")
      .mockResolvedValueOnce({ withdrawals: [], confirmed: false, complete: false, note: "reading chain history" } as never)
      .mockRejectedValue({ kind: "failed", message: "vault locked" });
    await store.refreshPendingWithdrawals(); // a retry at 2 s
    await store.refreshPendingWithdrawals(); // fails for good: no retry
    expect(read).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(5 * PENDING_CATCHUP_RETRY_MS);
    expect(read).toHaveBeenCalledTimes(2);
  });

  // TRIPWIRE: the pending-withdrawals read in the store never decides on the text of an error.
  it("tripwire: the store's pending-withdrawals read does not match error text", () => {
    const src = readFileSync(join(__dirname, "../shell/store.ts"), "utf8");
    const start = src.indexOf("private async readPendingWithdrawals(");
    expect(start).toBeGreaterThan(-1);
    const end = src.indexOf("\n  }\n", start);
    const body = src.slice(start, end);
    expect(textMatches(body)).toEqual([]);
    // Negative control: the ff96e43 shape is caught.
    expect(textMatches('if (msg.includes("still reading chain history")) {}')).not.toEqual([]);
    expect(textMatches("if (/indexing/.test(msg)) {}")).not.toEqual([]);
  });

  // TRIPWIRE (#371 Codex @31c065f): the read schedules its retry only through the one timer slot,
  // which is cleared before it is set and when a fresh read starts.
  it("tripwire: pending-withdrawal retries go through one timer slot", () => {
    const src = readFileSync(join(__dirname, "../shell/store.ts"), "utf8");
    expect(retryTimerViolations(src)).toEqual([]);
    // Negative control: the 31c065f shape.
    const old = "async refreshPendingWithdrawals() {\n  }\n  private async readPendingWithdrawals() {\n    setTimeout(() => void this.refreshPendingWithdrawals(), 2_000);\n  }\n";
    expect(retryTimerViolations(old)).toEqual([
      "readPendingWithdrawals sets its own timer",
      "schedulePendingRetry does not replace the pending timer",
      "a fresh read does not clear the pending timer",
    ]);
  });
});

/** Ways of deciding on the words of a message. */
function textMatches(code: string): string[] {
  return [/\.includes\(/, /\.startsWith\(/, /\.endsWith\(/, /\.match\(/, /\.test\(/, /\.indexOf\(/, /\.search\(/]
    .filter((re) => re.test(code))
    .map(String);
}

/** Where the store's pending-withdrawals retries escape the single timer slot. */
function retryTimerViolations(src: string): string[] {
  const body = (name: string) => {
    const start = src.indexOf(name);
    return start < 0 ? null : src.slice(start, src.indexOf("\n  }\n", start));
  };
  const bad: string[] = [];
  const read = body("private async readPendingWithdrawals(");
  if (read === null) bad.push("no readPendingWithdrawals");
  else if (read.includes("setTimeout(")) bad.push("readPendingWithdrawals sets its own timer");
  const schedule = body("private schedulePendingRetry(");
  const clear = body("private clearPendingRetry(");
  if (schedule === null || !schedule.includes("this.clearPendingRetry()") || clear === null || !clear.includes("clearTimeout(")) {
    bad.push("schedulePendingRetry does not replace the pending timer");
  }
  const refresh = body("async refreshPendingWithdrawals(");
  if (refresh === null || !refresh.includes("clearPendingRetry()")) bad.push("a fresh read does not clear the pending timer");
  return bad;
}
