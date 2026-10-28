// citrate-core#556 (Codex P2) — a rewards read that is still in flight must never overwrite a newer
// state. refreshEarnings reads ValidatorRegistry.rewardsOf(key) for the member's validator key (C1-V5).
// If the grant poll later reads the bond key as unread (a transient getter failure) and the Node /
// Wallet effect clears the figures with the unread-key reason, the older read must not restore its
// figures when it lands, and an older read that fails must not replace newer figures or the reason.
// Only the newest refreshEarnings call writes, and only while the member-key rule still names the key
// it read.
import { beforeEach, describe, expect, it, vi } from "vitest";

const validatorEarnings = vi.fn();
vi.mock("../bridge", () => ({
  bindSimHost: () => {},
  bridge: {
    mode: "tauri",
    node: { validatorEarnings: (...a: unknown[]) => validatorEarnings(...a) },
  },
}));
vi.mock("../bridge/mode", () => ({ BRIDGE_MODE: "tauri", assertSimAllowed: () => {} }));

import { Store } from "./store";
import type { AppState } from "./state";

const BOND_KEY = "0x" + "aa".repeat(32);
const OTHER_KEY = "0x" + "cc".repeat(32);
const OLD = { totalWei: "30000000000000000000", claimableWei: "20000000000000000000" };
const NEW = { totalWei: "40000000000000000000", claimableWei: "25000000000000000000" };
const UNREAD_KEY = "bond's validator key isn't read yet";

type Earnings = { totalWei: string; claimableWei: string; proposerPubkey: string };
function deferred() {
  let resolve!: (v: Earnings) => void;
  let reject!: (e: Error) => void;
  const promise = new Promise<Earnings>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function bondStore(pubkey: string | null = BOND_KEY): Store {
  const store = new Store();
  const s: Partial<AppState> = { hasGrant: true, bondActivated: true, bondPubkey: pubkey, bondedStake: 32_000, rewards: null, rewardsError: null };
  store.setState(s);
  return store;
}

beforeEach(() => {
  validatorEarnings.mockReset();
});

describe("#556 Codex P2 — an outstanding rewards read never overwrites a newer state", () => {
  it("a read that lands after the key went unread leaves the figures cleared, with the unread-key reason", async () => {
    const first = deferred();
    validatorEarnings.mockReturnValueOnce(first.promise);
    const store = bondStore();
    const pending = store.refreshEarnings();
    expect(validatorEarnings).toHaveBeenCalledWith(BOND_KEY);

    // The grant poll reads bondPubkey as null (transient getter failure); the effect re-runs.
    store.setState({ bondPubkey: null });
    await store.refreshEarnings();
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain(UNREAD_KEY);

    first.resolve({ ...OLD, proposerPubkey: BOND_KEY });
    await pending;
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain(UNREAD_KEY);
  });

  it("a read that fails after the key went unread does not replace the unread-key reason, and raises no toast", async () => {
    const first = deferred();
    validatorEarnings.mockReturnValueOnce(first.promise);
    const store = bondStore();
    const pending = store.refreshEarnings();
    store.setState({ bondPubkey: null });
    await store.refreshEarnings();

    first.reject(new Error("rpc timeout"));
    await pending;
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain(UNREAD_KEY);
    expect(String(store.state.toast ?? "")).not.toContain("rpc timeout");
  });

  it("an older read that fails after a newer read succeeded keeps the newer figures", async () => {
    const first = deferred();
    validatorEarnings.mockReturnValueOnce(first.promise).mockResolvedValueOnce({ ...NEW, proposerPubkey: BOND_KEY });
    const store = bondStore();
    const pending = store.refreshEarnings();
    await store.refreshEarnings();
    expect(store.state.rewards).toEqual(NEW);

    first.reject(new Error("rpc timeout"));
    await pending;
    expect(store.state.rewards).toEqual(NEW);
    expect(store.state.rewardsError).toBeNull();
    expect(String(store.state.toast ?? "")).not.toContain("rpc timeout");
  });

  it("an older read that succeeds after a newer one keeps the newer figures", async () => {
    const first = deferred();
    validatorEarnings.mockReturnValueOnce(first.promise).mockResolvedValueOnce({ ...NEW, proposerPubkey: BOND_KEY });
    const store = bondStore();
    const pending = store.refreshEarnings();
    await store.refreshEarnings();

    first.resolve({ ...OLD, proposerPubkey: BOND_KEY });
    await pending;
    expect(store.state.rewards).toEqual(NEW);
  });

  it("a read that lands after the stake and bond reads say there are no validator rewards writes nothing back", async () => {
    const first = deferred();
    validatorEarnings.mockReturnValueOnce(first.promise);
    const store = bondStore();
    const pending = store.refreshEarnings();
    store.setState({ hasGrant: false, bondActivated: false, bondPubkey: null, bondedStake: 0 });
    await store.refreshEarnings();
    expect(store.state.rewards).toBeNull();

    first.resolve({ ...OLD, proposerPubkey: BOND_KEY });
    await pending;
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toBeNull();
  });

  it("with no newer call, a read is still not committed once the current key rule no longer names its key", async () => {
    // The state changed but nothing has re-run refreshEarnings yet (e.g. neither Node nor Wallet is mounted).
    const first = deferred();
    validatorEarnings.mockReturnValueOnce(first.promise);
    const store = bondStore();
    const pending = store.refreshEarnings();
    store.setState({ bondPubkey: null });

    first.resolve({ ...OLD, proposerPubkey: BOND_KEY });
    await pending;
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).toContain(UNREAD_KEY);
  });

  it("with no newer call, a read for the old bond key is not committed after the key reads as another key", async () => {
    const first = deferred();
    validatorEarnings.mockReturnValueOnce(first.promise);
    const store = bondStore();
    const pending = store.refreshEarnings();
    store.setState({ bondPubkey: OTHER_KEY });

    first.resolve({ ...OLD, proposerPubkey: BOND_KEY });
    await pending;
    expect(store.state.rewards).toBeNull();
    expect(store.state.rewardsError).not.toBeNull();
  });

  it("the newest read still commits its figures (no regression)", async () => {
    validatorEarnings.mockResolvedValueOnce({ ...OLD, proposerPubkey: BOND_KEY });
    const store = bondStore();
    await store.refreshEarnings();
    expect(store.state.rewards).toEqual(OLD);
    expect(store.state.rewardsError).toBeNull();
  });
});
