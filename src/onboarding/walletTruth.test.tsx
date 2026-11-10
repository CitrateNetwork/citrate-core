// core#425 (S4 card) and the wallet copy around it, against the UI truth contract (rule 5: no copy
// ahead of the chain). In 0.5.2 the wallet is an EVM signing key sealed in this device's custody
// vault. There is no smart account (ERC-4337), no passkey signer, no guardian recovery, no UserOp
// and no lazy deployment, and losing the device loses the wallet (O-21). These render the screens a
// packaged member sees and check what they say. Written red-first against 9773d25f.
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

// Packaged-app mode: what a member of a release build sees.
vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

import { S4, S5 } from "./Onboarding";
import { Wallet } from "../surfaces/Wallet";
import { freshState, type AppState } from "../shell/state";
import type { Store } from "../shell/store";

const DEVICE = "0xD0D0000000000000000000000000000000000E0A";
const CLAIM = "0x" + "1".repeat(40);

/** A claim that the wallet is something 0.5.2 does not have. */
const NOT_IN_052 =
  /smart[ -]?(wallet|account)|ERC-?4337|passkey|WebAuthn|guardian|UserOp|deploys? lazily|counterfactual|EntryPoint|\/aa\/address/i;
/** A claim that someone else pays this wallet's gas. */
const SPONSORED = /sponsor/i;

function stubStore(linked: boolean): Store {
  return {
    identity: () => ({
      name: "Member",
      initials: "M",
      email: "member@example.com",
      sub: "sub-1",
      wallet: CLAIM,
      tier: "pilot",
      role: "member",
      org: null,
      real: true,
    }),
    walletIsLinked: () => linked,
    copy: () => {},
    setState: () => {},
    save: () => {},
  } as unknown as Store;
}

function signedIn(over: Partial<AppState> = {}): AppState {
  const s = freshState("p1");
  s.signedIn = true;
  s.walletFromClaim = true;
  return Object.assign(s, over);
}

describe("onboarding S4: the wallet card says what the wallet is", () => {
  const linked = renderToStaticMarkup(<S4 store={stubStore(true)} s={signedIn({ walletAddr: DEVICE, custodyAddr: DEVICE })} />);

  it("claims no smart account, passkey, guardian or lazy deployment", () => {
    expect(linked).not.toMatch(NOT_IN_052);
    expect(linked).not.toMatch(SPONSORED);
  });

  it("names the device key and says that losing the device loses the wallet", () => {
    expect(linked).toContain("Wallet key on this device");
    expect(linked).toContain(DEVICE);
    expect(linked).toContain("linked to your identity");
    expect(linked).toMatch(/no recovery/i);
    expect(linked).toMatch(/lost with it/i);
  });

  it("shows the device key, never the identity claim, when the two are not linked", () => {
    const html = renderToStaticMarkup(<S4 store={stubStore(false)} s={signedIn({ walletAddr: CLAIM, custodyAddr: DEVICE })} />);
    expect(html).toContain(DEVICE);
    expect(html).not.toContain(CLAIM);
    expect(html).toContain("not linked to your identity yet");
  });

  it("an unread device key is shown as unknown, not as the claim address", () => {
    const html = renderToStaticMarkup(<S4 store={stubStore(false)} s={signedIn({ walletAddr: CLAIM, custodyAddr: "" })} />);
    expect(html).toContain("device key not read yet");
    expect(html).not.toContain(CLAIM);
    expect(html).not.toContain("Copy");
  });
});

describe("onboarding S5: the grant ceremony claims no UserOp or sponsored transaction", () => {
  for (const phase of ["idle", "settling"] as const) {
    it(`phase ${phase}`, () => {
      const html = renderToStaticMarkup(<S5 store={stubStore(true)} s={signedIn({ s5: phase, walletAddr: DEVICE, custodyAddr: DEVICE })} />);
      expect(html).not.toMatch(NOT_IN_052);
      expect(html).not.toMatch(SPONSORED);
    });
  }
});

describe("Wallet: Receive and the identity tab call the wallet a device key", () => {
  function wallet(tab: string, linked: boolean, over: Partial<AppState>) {
    const s = signedIn(over);
    s.wTab = tab;
    return renderToStaticMarkup(<Wallet store={stubStore(linked)} s={s} />);
  }

  it("Receive shows the device key and claims no smart wallet, lazy deployment or sponsored gas", () => {
    const html = wallet("overview", true, { walletAddr: DEVICE, custodyAddr: DEVICE });
    expect(html).not.toMatch(NOT_IN_052);
    expect(html).not.toMatch(SPONSORED);
    expect(html).toContain(DEVICE);
    expect(html).toMatch(/lost with it/i);
  });

  it("Receive never offers the identity claim address while it is not this device's key", () => {
    const html = wallet("overview", false, { walletAddr: CLAIM, custodyAddr: DEVICE });
    expect(html).toContain(DEVICE);
    expect(html).not.toContain(CLAIM);
    const unread = wallet("overview", false, { walletAddr: CLAIM, custodyAddr: "" });
    expect(unread).not.toContain(CLAIM);
    expect(unread).toContain("device key not read yet");
  });

  it("the identity tab labels the primary wallet without calling it a smart wallet", () => {
    const html = wallet("identity", true, { walletAddr: DEVICE, custodyAddr: DEVICE });
    expect(html).not.toMatch(NOT_IN_052);
  });
});
