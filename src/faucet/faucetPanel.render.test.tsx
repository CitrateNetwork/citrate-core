// HUP-S6.5: Settings → Budgets: the faucet section renders its honest states.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FaucetPanel, FaucetTopUp } from "./FaucetPanel";
import type { FaucetApi, FaucetStatus } from "./faucet";

const NOW = 1_790_856_000_000;
const WALLET = "0x00000000000000000000000000000000000000aa";

function status(patch: Partial<FaucetStatus> = {}): FaucetStatus {
  return {
    enabled: false,
    budget: null,
    wallet: WALLET,
    walletError: null,
    walletMatches: false,
    storeError: null,
    health: { reachable: true, ready: true, detail: "The faucet is up and can send a drip." },
    nextEligibleAtMs: null,
    faucetNextEligibleAtMs: null,
    faucetEligibilityKnown: true,
    ledger: [],
    faucetUrl: "https://faucet.citrate.ai",
    faucetPage: `https://faucet.citrate.ai/?address=${WALLET}`,
    deployGasLimit: 2_000_000,
    dripWei: "10000000000000000000",
    windowHours: 24,
    maxPerWindow: 1,
    pendingOwnerSignOff: ["O-4: off by default. Pending owner sign-off."],
    nowMs: NOW,
    ...patch,
  };
}

const noopApi: FaucetApi = {
  status: async () => status(),
  grant: async () => ({ wallet: WALLET, grantedAtMs: NOW, windowMs: 86_400_000, maxPerWindow: 1 }),
  revoke: async () => undefined,
  request: async () => ({
    state: "disabled",
    outcome: null,
    txHash: null,
    nextEligibleAtMs: null,
    balanceWei: null,
    needWei: null,
    message: "x",
    faucetPage: "",
  }),
  openChallenge: async () => "",
};

describe("Feature: the faucet is off by default and says so", () => {
  it("off: no request is made, the switch offers Turn on, sign-off items are listed", () => {
    const html = renderToStaticMarkup(<FaucetPanel initial={status()} api={noopApi} />);
    expect(html).toMatch(/Off\. Core never asks the faucet for you\./);
    expect(html).toMatch(/Turn on/);
    expect(html).not.toMatch(/Turn off/);
    expect(html).toMatch(/10 SALT/);
    expect(html).toMatch(/Pending owner sign-off/);
    expect(html).not.toMatch(/\u2014/);
  });

  it("on for this wallet: shows the budget and offers Turn off", () => {
    const html = renderToStaticMarkup(
      <FaucetPanel
        initial={status({
          enabled: true,
          walletMatches: true,
          budget: { wallet: WALLET, grantedAtMs: NOW, windowMs: 86_400_000, maxPerWindow: 1 },
        })}
        api={noopApi}
      />,
    );
    expect(html).toMatch(/On for 0x0000…00aa: at most 1 top-up every 24 hours/);
    expect(html).toMatch(/Turn off/);
  });

  it("granted for another wallet: it does nothing and says why", () => {
    const html = renderToStaticMarkup(
      <FaucetPanel
        initial={status({
          enabled: true,
          walletMatches: false,
          budget: { wallet: "0x00000000000000000000000000000000000000bb", grantedAtMs: NOW, windowMs: 86_400_000, maxPerWindow: 1 },
        })}
        api={noopApi}
      />,
    );
    expect(html).toMatch(/not your current wallet/);
    expect(html).toMatch(/Turn on for this wallet/);
  });
});

describe("Feature: unreachable faucet, waiting, and history", () => {
  it("an unreachable faucet is shown plainly, with the next eligible time and the request rows", () => {
    const html = renderToStaticMarkup(
      <FaucetPanel
        initial={status({
          enabled: true,
          walletMatches: true,
          health: { reachable: false, ready: null, detail: "connection refused" },
          nextEligibleAtMs: NOW + 3_600_000,
          ledger: [
            {
              atMs: NOW - 1000,
              wallet: WALLET,
              origin: "hermes",
              initcodeHash: "0x" + "ab".repeat(32),
              needWei: "2000000000000000",
              balanceWei: "0",
              outcome: "sent",
              txHash: "0x" + "5e".repeat(32),
              message: "Successfully sent 10 SALT",
              nextEligibleAtMs: null,
            },
            {
              atMs: NOW - 2000,
              wallet: WALLET,
              origin: "local-user",
              initcodeHash: null,
              needWei: null,
              balanceWei: null,
              outcome: "rate_limited",
              txHash: null,
              message: "Rate limited: ip cooldown: 0h 40m remaining",
              nextEligibleAtMs: NOW,
            },
          ],
        })}
        api={noopApi}
      />,
    );
    expect(html).toMatch(/Faucet unreachable\. connection refused/);
    expect(html).toMatch(/Next top-up possible/);
    expect((html.match(/data-testid="faucet-ledger-row"/g) ?? []).length).toBe(2);
    expect(html).toMatch(/sent · asked by hermes · tx 0x5e5e5e5e/);
    expect(html).toMatch(/refused for now \(limit\) · asked by local-user · Rate limited/);
  });

  it("outside the desktop app the panel says where the faucet lives", () => {
    const html = renderToStaticMarkup(<FaucetPanel api={null} />);
    expect(html).toMatch(/available in the Citrate Core desktop app/);
  });
});

describe("Feature: the top-up button under a READY deploy", () => {
  it("renders in the desktop app and nothing outside it", () => {
    expect(renderToStaticMarkup(<FaucetTopUp initcodeHash={"0x" + "ab".repeat(32)} api={null} />)).toBe("");
    const html = renderToStaticMarkup(<FaucetTopUp initcodeHash={"0x" + "ab".repeat(32)} api={noopApi} />);
    expect(html).toMatch(/Short of gas\? Ask the faucet/);
  });
});
