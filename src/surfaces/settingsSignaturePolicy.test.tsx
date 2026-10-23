// Settings › Keys & security, against the UI truth contract and CLAUDE.md Rule 3.
//
// 1. Signature policy. Every signature goes through the signature ceremony. The one exception is a
//    budgeted web sign-in (SIWE), granted, capped and revocable in Budgets. There is no
//    "allowlisted intents" policy, so Settings offers no control that writes one, and a value an
//    older build stored (config.json or the persisted app state) can neither surface nor be read.
// 2. The wallet row names what the wallet is: a secp256k1 key on this device, sealed in the
//    custody vault, signing through the ceremony. Not a passkey validator, not a lazily deployed
//    smart wallet.
import { describe, it, expect, beforeEach } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { Settings } from "./Settings";
import { freshState, loadState, PERSIST_KEYS, STORAGE_KEY, type AppState } from "../shell/state";
import { DEFAULT_APP_CONFIG } from "../bridge/types";
import type { Store } from "../shell/store";

const stubStore = {
  identity: () => ({
    name: "Member",
    initials: "M",
    email: "member@example.com",
    sub: "sub-1",
    wallet: "0xC0ffee0000000000000000000000000000ABCDEF",
    tier: "pilot",
    role: "member",
    org: null,
    real: true,
  }),
} as unknown as Store;

const DEVICE = "0xD0D0000000000000000000000000000000000E0A";

function keysState(extra: Record<string, unknown> = {}): AppState {
  const s = freshState("p1");
  s.sSec = "keys";
  s.signedIn = true;
  // `extra` may carry keys AppState no longer has (a value an older build persisted).
  return Object.assign({ ...s }, extra) as AppState;
}

const render = (s: AppState) => renderToStaticMarkup(<Settings store={stubStore} s={s} />);

describe("Settings › signature policy: only the budgeted web sign-in skips the ceremony", () => {
  it("offers no allowlist policy control", () => {
    const html = render(keysState());
    expect(html).not.toMatch(/allowlisted intents skip the ceremony/i);
    expect(html).not.toContain("Edit allowlist rules");
    expect(html).not.toContain("Approve every signature");
  });

  it("states the rule and the one budgeted exception", () => {
    const html = render(keysState());
    expect(html).toContain("Every signature stops at the signature ceremony");
    expect(html).toContain("web sign-in (SIWE) inside a budget you grant");
  });

  it("a legacy stored sigPolicy 'allow' changes nothing on screen (fails closed)", () => {
    const html = render(keysState({ sigPolicy: "allow" }));
    expect(html).not.toMatch(/allowlisted intents/i);
    expect(html).not.toContain("Edit allowlist rules");
    expect(html).toContain("Every signature stops at the signature ceremony");
  });

  it("a garbage stored sigPolicy changes nothing on screen either", () => {
    for (const v of ["ALLOW", "skip", "", null, 1, { allow: true }]) {
      const html = render(keysState({ sigPolicy: v }));
      expect(html).not.toMatch(/allowlist/i);
      expect(html).toContain("Every signature stops at the signature ceremony");
    }
  });
});

describe("persisted state: an old signature policy is dropped on load", () => {
  beforeEach(() => localStorage.clear());

  it("the app config contract carries no signature policy", () => {
    expect(Object.keys(DEFAULT_APP_CONFIG)).not.toContain("sigPolicy");
  });

  it("the app state does not persist a signature policy", () => {
    expect(PERSIST_KEYS as string[]).not.toContain("sigPolicy");
    expect(Object.keys(freshState("p1"))).not.toContain("sigPolicy");
  });

  it("loadState drops a stored sigPolicy 'allow' instead of restoring it", () => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify({ persona: "p1", sigPolicy: "allow", autolock: 15 }));
    const st = loadState();
    expect(Object.keys(st)).not.toContain("sigPolicy");
    // The rest of the saved state still loads.
    expect(st.autolock).toBe(15);
  });
});

describe("Settings › keystore: the wallet row says what the wallet is", () => {
  it("does not call it a passkey validator or a lazily deployed smart wallet", () => {
    const html = render(keysState());
    expect(html).not.toMatch(/passkey validator/i);
    expect(html).not.toContain("WebAuthn P-256");
    expect(html).not.toContain("deploys lazily");
    expect(html).not.toContain("Smart wallet");
  });

  it("names the device key and shows the device key's address", () => {
    const html = render(keysState({ custodyAddr: DEVICE, walletAddr: DEVICE }));
    expect(html).toContain("Wallet key on this device");
    expect(html).toContain("secp256k1");
    expect(html).toContain("0xD0D0…0E0A");
    expect(html).toContain("linked to your identity");
    expect(html).not.toContain("not linked");
  });

  it("an unlinked device key says so, and an unread device key shows — (unknown is not a key)", () => {
    const unlinked = render(keysState({ custodyAddr: DEVICE, walletAddr: "0x" + "1".repeat(40) }));
    expect(unlinked).toContain("not linked to your identity yet");
    const none = render(keysState({ custodyAddr: "", walletAddr: "0x" + "1".repeat(40) }));
    expect(none).toContain("device key not read yet");
    expect(none).not.toMatch(/linked to your identity/);
    // The identity claim address is not passed off as this device's key.
    expect(none).not.toContain("0x1111…1111");
  });
});
