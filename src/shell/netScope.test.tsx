// RC52 (UI truth contract): a 0.5.1 install on r1005 started on a new genesis (K6, #363; the
// cross-genesis fixture is #499's). The webview's saved state holds values read from the r1005
// chain: the grant (settled, 32,000), the bond, balances, claimable and the r1005 tx history.
// None of them may show as current on the new genesis. Each is read again from the new chain or
// shown as "not read yet"; the old values are archived as the previous network's, never deleted.
//
// The saved state is the one v0.5.1 itself wrote (fixtures/upgrade-v0.5.1/webview); the old
// genesis is the 0.5.1 node marker of #499's fixture (node-r1005/chain-genesis); the new genesis
// is the one #499's Rust test starts the node on (upgrade_xgenesis_tests.rs NEW_GENESIS).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";
import { freshState, loadState, STORAGE_KEY, type AppState } from "./state";
import { readNetArchive, NET_ARCHIVE_KEY } from "./netScope";
import { store, type Store } from "./store";
import { bridge } from "../bridge";
import { Wallet } from "../surfaces/Wallet";
import { Node } from "../surfaces/Node";
import { scanTxUrl, scanAddrUrl } from "../data/links";
import { ACTIVE_PROFILE, type NetworkProfile } from "../network/profile";
import type { GrantStatus } from "../bridge/domains";

const ROOT = join(__dirname, "../..");
const MEMBER = JSON.parse(
  readFileSync(join(ROOT, "fixtures/upgrade-v0.5.1/webview/localStorage.citrate-core-proto-v2.member.json"), "utf8"),
) as Record<string, unknown>;
/** The genesis the 0.5.1 install's node was on (#499 fixture marker). */
const OLD_GENESIS = readFileSync(join(ROOT, "fixtures/upgrade-v0.5.1/node-r1005/chain-genesis"), "utf8").split("\n")[0].trim();
/** The genesis #499's Rust test starts that install on. */
const NEW_GENESIS = (() => {
  const src = readFileSync(join(ROOT, "src-tauri/src/upgrade_xgenesis_tests.rs"), "utf8");
  const m = /const NEW_GENESIS: &str = "(0x[0-9a-f]{64})";/.exec(src);
  if (!m) throw new Error("upgrade_xgenesis_tests.rs no longer names NEW_GENESIS");
  return m[1];
})();

/** Every value the 0.5.1 save holds that was read from the r1005 chain. Listed by hand, not taken
 *  from the code under test. (Its earnVal / earnPin / earnComp / earnToday are retired keys, which
 *  loadState drops on any network: RETIRED_KEYS in state.ts.) */
const R1005_VALUES = [
  "hasGrant", "hasSbt", "s5", "s5n", "s5StakeWei", "s5BondStatus", "liquid", "claimable", "activity",
  "sponsorUnits",
] as const;

/** This install's own values: they survive any switch (UPGRADE_051_052 §1, §2.4, §2.8). */
const DEVICE_VALUES = [
  "walletAddr", "commsAddr", "deviceId", "telemetry", "rpc", "net", "cpuCap", "autolock", "channel", "dataDir",
  "jPages", "userSkills", "connections", "pins", "hermesSidecarLoop", "hermesTerminal", "hermesReadAloud",
  "aiDefault", "activeModelId", "kycOutcome", "s2", "s3", "stage", "crashes",
] as const;

function loadOn(blob: Record<string, unknown>, genesis: string): AppState {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(blob));
  return loadState(genesis);
}

const noGrant: GrantStatus = {
  attributedStakeWei: "0",
  attributedPrincipalWei: "0",
  hasSbt: false,
  bondAddress: "0x" + "b".repeat(40),
  bondDeployed: false,
  bondedStakeWei: "0",
  hasValidator: false,
  unlockBlock: null,
  isUnlocked: false,
  isKycVerified: false,
};

const stubStore = {
  walletIsLinked: () => true,
  identity: () => ({ name: "M", initials: "M", email: "—", sub: "—", wallet: MEMBER.walletAddr, tier: "member", role: "member", org: null, real: true }),
  refreshWallet: () => {},
  refreshActivity: () => {},
  refreshEarnings: () => {},
  toast: () => {},
  dismissNetworkNotice: () => {},
} as unknown as Store;

beforeEach(() => localStorage.clear());
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("a 0.5.1 install on r1005 started on a new genesis", () => {
  it("the fixture's two genesis values differ and the save is the r1005 member's", () => {
    expect(OLD_GENESIS).toMatch(/^0x[0-9a-f]{64}$/);
    expect(NEW_GENESIS).not.toBe(OLD_GENESIS);
    expect(MEMBER.hasGrant).toBe(true);
    expect((MEMBER.activity as unknown[]).length).toBe(2);
  });

  it("shows no r1005 network value as current", () => {
    const s = loadOn(MEMBER, NEW_GENESIS) as unknown as Record<string, unknown>;
    const fresh = freshState("p1") as unknown as Record<string, unknown>;
    for (const k of R1005_VALUES) {
      expect({ key: k, value: s[k] }).toEqual({ key: k, value: fresh[k] });
    }
    expect(s.hasGrant).toBe(false);
    expect(s.activity).toEqual([]);
  });

  it("marks every reset value as not read yet on the new chain", () => {
    const s = loadOn(MEMBER, NEW_GENESIS);
    expect([...s.netUnread].sort()).toEqual(["activity", "balances", "grant"]);
  });

  it("keeps this install's own values: wallet identity, settings, Hermes, journal", () => {
    const s = loadOn(MEMBER, NEW_GENESIS) as unknown as Record<string, unknown>;
    for (const k of DEVICE_VALUES) {
      expect({ key: k, value: s[k] }).toEqual({ key: k, value: MEMBER[k] });
    }
  });

  it("archives the r1005 values under the r1005 genesis instead of deleting them", () => {
    loadOn(MEMBER, NEW_GENESIS);
    const archive = readNetArchive();
    expect(archive).toHaveLength(1);
    expect(archive[0].genesis).toBe(OLD_GENESIS);
    for (const k of R1005_VALUES) expect({ key: k, value: archive[0].values[k] }).toEqual({ key: k, value: MEMBER[k] });
    expect(JSON.parse(localStorage.getItem(NET_ARCHIVE_KEY) ?? "null")).not.toBeNull();
  });

  it("raises the switch notice naming the previous genesis", () => {
    const s = loadOn(MEMBER, NEW_GENESIS);
    expect(s.networkNotice).toMatchObject({ from: OLD_GENESIS, to: NEW_GENESIS });
  });

  it("on the same genesis (r1005) nothing is reset, archived or noticed", () => {
    const s = loadOn(MEMBER, OLD_GENESIS) as unknown as Record<string, unknown>;
    for (const k of R1005_VALUES) expect({ key: k, value: s[k] }).toEqual({ key: k, value: MEMBER[k] });
    expect(s.netUnread).toEqual([]);
    expect(s.networkNotice).toBeNull();
    expect(readNetArchive()).toEqual([]);
  });
});

describe("saves are stamped with their genesis", () => {
  it("a save after the switch is stamped with the new genesis and reloads there unchanged", () => {
    vi.useFakeTimers();
    store.state = loadOn(MEMBER, NEW_GENESIS);
    store.save();
    vi.advanceTimersByTime(1000);
    const written = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Record<string, unknown>;
    expect(written.netGenesis).toBe(NEW_GENESIS);
    const again = loadState(NEW_GENESIS);
    expect(again.hasGrant).toBe(false);
    expect([...again.netUnread].sort()).toEqual(["activity", "balances", "grant"]);
    expect(readNetArchive()).toHaveLength(1);
  });

  it("a save stamped with another genesis is archived when loaded", () => {
    const s = loadOn({ ...MEMBER, netGenesis: NEW_GENESIS }, OLD_GENESIS);
    expect(s.hasGrant).toBe(false);
    expect(readNetArchive()[0].genesis).toBe(NEW_GENESIS);
  });

  it("a malformed stamp counts as an unknown network: archived, never shown as current", () => {
    for (const bad of [42, "0xzz", "", null]) {
      localStorage.clear();
      const s = loadOn({ ...MEMBER, netGenesis: bad }, OLD_GENESIS);
      expect({ bad, hasGrant: s.hasGrant }).toEqual({ bad, hasGrant: false });
      expect(readNetArchive()[0].genesis).toBeNull();
    }
  });

  it("a second switch keeps the first archive (append only)", () => {
    loadOn(MEMBER, NEW_GENESIS);
    const third = "0x" + "3".repeat(64);
    loadOn({ ...MEMBER, netGenesis: NEW_GENESIS, liquid: 7 }, third);
    const archive = readNetArchive();
    expect(archive.map((a) => a.genesis)).toEqual([OLD_GENESIS, NEW_GENESIS]);
    expect(archive[1].values.liquid).toBe(7);
  });

  it("a damaged archive is kept aside, not overwritten by the next switch", () => {
    localStorage.setItem(NET_ARCHIVE_KEY, "{not json");
    loadOn(MEMBER, NEW_GENESIS);
    const keys = Object.keys(localStorage).filter((k) => k.startsWith(NET_ARCHIVE_KEY + ".damaged-"));
    expect(keys).toHaveLength(1);
    expect(localStorage.getItem(keys[0])).toBe("{not json");
    expect(readNetArchive()).toHaveLength(1);
  });
});

describe("the store re-reads the grant on the new chain", () => {
  it("reads grantStatus after the switch and clears 'not read yet' when there is no grant there", async () => {
    store.state = loadOn(MEMBER, NEW_GENESIS);
    const grant = vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(noGrant);
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 1, height: 3, syncPct: 50 });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    await store.refreshNode();
    expect(grant).toHaveBeenCalled();
    expect(store.state.hasGrant).toBe(false);
    expect(store.state.netUnread).not.toContain("grant");
  });

  it("a failed balance read leaves the balances 'not read yet'; a good one clears it", async () => {
    store.state = loadOn(MEMBER, NEW_GENESIS);
    vi.spyOn(bridge.wallet, "balances").mockRejectedValueOnce(new Error("rpc down"));
    await store.refreshWallet();
    expect(store.state.netUnread).toContain("balances");
    vi.spyOn(bridge.wallet, "balances").mockResolvedValue({ liquid: 3, staked: 0, claimable: 0 } as Awaited<ReturnType<typeof bridge.wallet.balances>>);
    await store.refreshWallet();
    expect(store.state.netUnread).not.toContain("balances");
    expect(store.state.liquid).toBe(3);
  });

  it("the r1005 rows never come back through the activity merge", async () => {
    store.state = loadOn(MEMBER, NEW_GENESIS);
    vi.spyOn(bridge.wallet, "activity").mockResolvedValue([]);
    await store.refreshActivity();
    expect(store.state.activity).toEqual([]);
    expect(store.state.netUnread).not.toContain("activity");
  });
});

describe("what the member sees after the switch", () => {
  it("Wallet shows 'not read yet', never the r1005 grant or balance", () => {
    const s = { ...loadOn(MEMBER, NEW_GENESIS), wTab: "overview" };
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={s} />);
    expect(html).toContain("not read yet");
    expect(html).not.toContain("41.50");
    expect(html).not.toContain("32,000");
    const staking = renderToStaticMarkup(<Wallet store={stubStore} s={{ ...s, wTab: "staking" }} />);
    expect(staking).toContain("not read yet");
    // The grant row reads "not read yet" (the page's other 32,000s are the validator minimum).
    expect(staking).toMatch(/Membership grant · locked in your bond<\/span><span[^>]*>not read yet on this network</);
  });

  it("Wallet keeps the r1005 history under a previous-network label, with no explorer link", () => {
    const s = { ...loadOn(MEMBER, NEW_GENESIS), wTab: "activity" };
    const html = renderToStaticMarkup(<Wallet store={stubStore} s={s} />);
    expect(html).toContain("Previous network");
    expect(html).toContain(OLD_GENESIS.slice(0, 10));
    expect(html).toContain("Grant + stake ceremony");
    expect(html).not.toContain("Open on CitrateScan");
  });

  it("Node keeps the switch notice until it is dismissed", () => {
    const s = loadOn(MEMBER, NEW_GENESIS);
    const html = renderToStaticMarkup(<Node store={stubStore} s={s} />);
    expect(html).toContain('data-network-notice="switch"');
    expect(html).toContain(OLD_GENESIS.slice(0, 10));
    expect(html).toContain("Dismiss");
    const gone = renderToStaticMarkup(<Node store={stubStore} s={{ ...s, networkNotice: null }} />);
    expect(gone).not.toContain('data-network-notice="switch"');
  });

  it("the node's reset notice is kept in state (persisted) until dismissed, not only toasted", async () => {
    vi.useFakeTimers();
    store.state = loadOn(MEMBER, OLD_GENESIS);
    const NOTICE = "Citrate Network was upgraded to a new chain; your node is resyncing from the start";
    vi.spyOn(bridge.node, "status").mockResolvedValue({ state: "running", peers: 1, height: 3, syncPct: 1, notice: NOTICE });
    vi.spyOn(bridge.node, "logs").mockResolvedValue([]);
    vi.spyOn(bridge.membership, "grantStatus").mockResolvedValue(noGrant);
    await store.refreshNode();
    expect(store.state.networkNotice?.node).toBe(NOTICE);
    vi.advanceTimersByTime(10_000);
    expect(store.state.networkNotice?.node).toBe(NOTICE);
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Record<string, unknown>;
    expect((saved.networkNotice as { node: string }).node).toBe(NOTICE);
    store.dismissNetworkNotice();
    expect(store.state.networkNotice).toBeNull();
  });
});

describe("explorer links follow the active network profile", () => {
  it("links use the profile's explorer, and there is no link without one", () => {
    const p = { ...(ACTIVE_PROFILE as NetworkProfile), explorerUrl: "https://scan.example" };
    expect(scanTxUrl("0xab", p)).toBe("https://scan.example/tx/0xab");
    expect(scanAddrUrl("0xcd", p)).toBe("https://scan.example/address/0xcd");
    const none = { ...p, explorerUrl: null };
    expect(scanTxUrl("0xab", none)).toBeNull();
    expect(scanAddrUrl("0xcd", none)).toBeNull();
  });
});
