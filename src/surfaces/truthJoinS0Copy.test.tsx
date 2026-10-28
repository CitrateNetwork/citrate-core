// Join 3 copy review (fed#353, JOIN3_COPY_REVIEW.md) on onboarding S0:
//   V-01 (§5.4, §5.7): "One membership funds your wallet, covers your validator stake…" is wrong —
//        the grant goes into the member's bond, not the wallet, and is staked only after activation.
//        Proposed: "Citrate runs a node, a wallet and Hermes on this machine. Your keys stay here."
//        and, under the buttons, "Joining means a yearly membership and a bond. Explore free skips
//        both; you can upgrade later in Settings › Billing."
//   V-02 (§2 Network, §6): "Chain 40204 · testnet-beta" — chain id alone is not enough. Show
//        "Citrate betanet" once the genesis check passes, else "Network not checked yet" (and
//        "Update required" on a genesis mismatch).
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const block = vi.hoisted(() => ({ hash: undefined as string | undefined }));
vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));
vi.mock("wagmi", () => ({
  useBlockNumber: () => ({ data: undefined, isPending: true, error: null }),
  useReadContract: () => ({ data: undefined }),
  useBlock: () => ({ data: block.hash ? { hash: block.hash } : undefined }),
}));
vi.mock("../components/AgentChat", () => ({ AgentChat: () => null }));
vi.mock("../widgets/WidgetsPanel", () => ({ WidgetsPanel: () => null }));
vi.mock("../daemons/DaemonsPanel", () => ({ DaemonsPanel: () => null }));

import { Onboarding } from "../onboarding/Onboarding";
import type { Store } from "../shell/store";
import { freshState } from "../shell/state";
import { EXPECTED_GENESIS, networkLabel } from "../shell/networkCompat";

const stub = { onJoin: () => {}, onExplore: () => {} } as unknown as Store;
const s0 = () => renderToStaticMarkup(<Onboarding store={stub} s={{ ...freshState("p1"), stage: "s0" }} />);

describe("Join 3 V-01 — S0 opening line", () => {
  it("does not say the membership funds the wallet or covers the stake", () => {
    block.hash = EXPECTED_GENESIS;
    const html = s0();
    expect(html).not.toContain("funds your wallet");
    expect(html).not.toContain("covers your validator stake");
  });

  it("uses the reviewed wording, and says what Join involves under the buttons", () => {
    const html = s0();
    expect(html).toContain("Citrate runs a node, a wallet and Hermes on this machine. Your keys stay here.");
    // #576 (owner decision 2026-10-27, option a): Join sets up a free node first, so the line under
    // the buttons no longer says joining means a membership and a bond.
    expect(html).toContain("Membership and its bond are optional. You can upgrade later in Settings › Billing.");
    expect(html).not.toContain("Joining means a yearly membership");
  });
});

describe("Join 3 V-02 — the network label", () => {
  it("is 'Citrate betanet' only after the genesis check passes", () => {
    expect(networkLabel("compatible")).toBe("Citrate betanet");
    expect(networkLabel("unknown")).toBe("Network not checked yet");
    expect(networkLabel("retired")).toBe("Update required");
  });

  it("S0 shows 'Citrate betanet' for this build's genesis and 'Network not checked yet' before the read", () => {
    block.hash = EXPECTED_GENESIS;
    expect(s0()).toContain("Citrate betanet");
    block.hash = undefined;
    const unread = s0();
    expect(unread).toContain("Network not checked yet");
    expect(unread).not.toContain("Citrate betanet");
    expect(unread).not.toContain("testnet-beta");
  });
});

// Tripwire: neither phrase comes back in any user-visible source (comments are ignored).
describe("tripwire — Join 3 V-01 / V-02 phrases", () => {
  it("no non-test source says 'funds your wallet' or 'testnet-beta' outside comments", () => {
    const files: string[] = [];
    const walk = (d: string) => {
      for (const n of readdirSync(d)) {
        const p = join(d, n);
        if (statSync(p).isDirectory()) walk(p);
        else if (/\.(ts|tsx)$/.test(n) && !/\.test\.tsx?$/.test(n)) files.push(p);
      }
    };
    walk("src");
    const hits: string[] = [];
    for (const f of files) {
      readFileSync(f, "utf8")
        .split("\n")
        .forEach((line, i) => {
          if (/^\s*(\/\/|\*|\/\*|\{\/\*)/.test(line)) return;
          if (/funds your wallet|testnet-beta/i.test(line)) hits.push(`${f}:${i + 1}`);
        });
    }
    expect(hits).toEqual([]);
  });
});
