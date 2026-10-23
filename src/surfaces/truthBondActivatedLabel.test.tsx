// core#407. UI truth contract v1.1 §1 Validators: "Bond locked · not staked" means
// `MemberBond.activated()` = false. After a full exit or a slash the registry stake reads 0 and
// pubkeyOfStaker is cleared, but the bond stays activated (and `hasGrant` stays true), so the grant
// and a zero stake are not evidence of a locked, un-staked bond. Wallet, Dashboard and the bond
// status line take the word from the one validator state (`validatorWord` / `validatorDash`) or
// from `bondActivated === false`, and say Exiting / Slashed / "—" otherwise.
import { describe, it, expect, vi } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { renderToStaticMarkup } from "react-dom/server";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Dashboard } from "./Dashboard";
import { Wallet } from "./Wallet";
import { describeBond, type Store } from "../shell/store";
import { freshState, type AppState } from "../shell/state";
import type { ValidatorActivation } from "../shell/chainTruth";

const WEI = 10n ** 18n;
const LOCKED = /bond locked · not staked/i;

function stubStore(): Store {
  return {
    walletIsLinked: () => true,
    refreshEarnings: async () => {},
    refreshWallet: () => {},
    refreshPendingWithdrawals: () => {},
    refreshActivity: () => {},
    refreshChainParams: () => {},
    refreshProducers: () => {},
    toast: () => {},
    identity: () => ({ sub: "sub-1", wallet: "0x" + "0".repeat(40) }),
  } as unknown as Store;
}

const bond = (activated: boolean | null) => ({
  deployed: true,
  activated,
  pubkey: "0x" + "22".repeat(32),
  unlocked: false,
});
const act = (status: number): ValidatorActivation => ({
  isActive: false,
  status,
  actEpoch: 500,
  currentEpoch: 546,
});

// A granted member whose registry stake reads 0.
function member(over: Partial<AppState>): AppState {
  return {
    ...freshState("p1"),
    signedIn: true,
    node: "synced",
    peers: 3,
    syncPct: 100,
    height: 546_000,
    hasGrant: true,
    s5StakeWei: (32_000n * WEI).toString(),
    bondedStake: 0,
    ...over,
  };
}

function surfaces(s: AppState): string[] {
  const st = stubStore();
  return [
    renderToStaticMarkup(<Wallet store={st} s={{ ...s, wTab: "overview" }} />),
    renderToStaticMarkup(<Wallet store={st} s={{ ...s, wTab: "staking" }} />),
    renderToStaticMarkup(<Dashboard store={st} s={s} />),
  ];
}

describe("core#407 — an activated bond with zero stake is never 'Bond locked · not staked'", () => {
  it("after an exit the Wallet and Dashboard say Exiting", () => {
    for (const html of surfaces(
      member({ memberBond: bond(true), validatorActivation: act(2) }),
    )) {
      expect(html).not.toMatch(LOCKED);
      expect(html).toContain("Exiting");
    }
  });

  it("after a slash the Wallet and Dashboard say Slashed", () => {
    for (const html of surfaces(
      member({ memberBond: bond(true), validatorActivation: act(3) }),
    )) {
      expect(html).not.toMatch(LOCKED);
      expect(html).toContain("Slashed");
    }
  });

  it("an activated bond with the registry unread, or reading None, is not the locked word", () => {
    for (const validatorActivation of [null, act(0)]) {
      for (const html of surfaces(
        member({ memberBond: bond(true), validatorActivation }),
      )) {
        expect(html).not.toMatch(LOCKED);
      }
    }
  });

  it("an unread activated() flag (or no bond read) is not the locked word", () => {
    for (const memberBond of [bond(null), null]) {
      for (const validatorActivation of [null, act(0)]) {
        for (const html of surfaces(
          member({ memberBond, validatorActivation }),
        )) {
          expect(html).not.toMatch(LOCKED);
        }
      }
    }
  });

  it("control: a bond that reads not activated, status None, is still 'Bond locked · not staked'", () => {
    for (const validatorActivation of [null, act(0)]) {
      for (const html of surfaces(
        member({ memberBond: bond(false), validatorActivation }),
      )) {
        expect(html).toMatch(LOCKED);
      }
    }
  });
});

describe("core#407 — the bond status line (describeBond) needs bondActivated === false", () => {
  const base = {
    bondDeployed: true,
    unlockBlock: 300_000,
    isUnlocked: false,
    isKycVerified: false,
    hasValidator: false,
  };

  it("an exited or slashed member's activated bond is locked, not 'not staked'", () => {
    expect(describeBond({ ...base, bondActivated: true })).toBe(
      "Locked until block 300,000",
    );
  });

  it("an unread activated() flag is locked, not 'not staked'", () => {
    expect(describeBond({ ...base, bondActivated: null })).toBe(
      "Locked until block 300,000",
    );
    expect(describeBond(base)).toBe("Locked until block 300,000");
  });

  it("control: a bond that reads not activated is 'Bond locked · not staked'", () => {
    expect(describeBond({ ...base, bondActivated: false })).toBe(
      "Bond locked · not staked · until block 300,000",
    );
  });
});

// Tripwire: the word is produced in exactly two places, both gated on `activated === false`:
// chainTruth.ts validatorState (the ValidatorWord type and the bondNotActivated branch) and store.ts
// describeBond. Any other source file that spells it is deriving it from something else (a grant, a
// zero stake). Comparing validatorWord's result with it (`=== "Bond locked · not staked"`) is fine.
describe("core#407 tripwire — 'Bond locked · not staked' has one derivation", () => {
  const SRC = join(__dirname, "..");
  const ALLOWED: Record<string, number> = {
    "shell/chainTruth.ts": 2,
    "shell/store.ts": 1,
  };

  function files(dir: string): string[] {
    return readdirSync(dir).flatMap((f) => {
      const p = join(dir, f);
      if (statSync(p).isDirectory()) return files(p);
      return /\.(ts|tsx)$/.test(f) && !/\.test\.tsx?$/.test(f) ? [p] : [];
    });
  }

  function codeHits(text: string): number {
    let n = 0;
    for (const raw of text.split("\n")) {
      const t = raw.trim();
      if (t.startsWith("//") || t.startsWith("*") || t.startsWith("/*"))
        continue;
      // A comparison against validatorWord's result consumes the word; it doesn't produce it.
      const code = raw
        .replace(/\s\/\/\s.*$/, "")
        .replace(/===\s*"Bond locked · not staked"/g, "");
      n += (code.match(/bond locked · not staked/gi) ?? []).length;
    }
    return n;
  }

  it("only chainTruth.validatorState and describeBond spell the word", () => {
    const found: Record<string, number> = {};
    for (const f of files(SRC)) {
      const n = codeHits(readFileSync(f, "utf8"));
      if (n > 0) found[f.slice(SRC.length + 1)] = n;
    }
    expect(found).toEqual(ALLOWED);
  });

  it("describeBond's not-staked branch is gated on bondActivated === false", () => {
    const src = readFileSync(join(SRC, "shell/store.ts"), "utf8");
    const line = src.split("\n").find((l) => codeHits(l) > 0) ?? "";
    // #349 derives `activated` from bondActivated (a registry binding implies it); either spelling.
    expect(line).toMatch(/\b(bondActivated|activated) === false/);
    expect(src).toMatch(/const activated = g\.hasValidator \? true : \(g\.bondActivated \?\? null\);/);
  });
});
