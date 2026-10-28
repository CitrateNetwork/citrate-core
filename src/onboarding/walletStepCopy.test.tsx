// core#399 found the wallet step (onboarding S4) makes the same false claim the Settings wallet card
// did: "Smart wallet · ERC-4337" and a "Passkey validator (WebAuthn P-256)" signer. The wallet is an
// EVM signing key created on this device, sealed in the custody vault, that signs only through the
// signature ceremony. #399's wording: "Wallet key on this device"; "EVM signing key · sealed in the
// custody vault"; it signs only through the signature ceremony (since #545 a row of its own).
import { describe, it, expect, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { readFileSync } from "node:fs";

vi.mock("../bridge/mode", () => ({
  BRIDGE_MODE: "tauri" as const,
  assertSimAllowed: () => {},
}));

import { S4 } from "./Onboarding";
import type { Store } from "../shell/store";
import { freshState } from "../shell/state";

const stub = { copy: () => {}, setState: () => {}, save: () => {} } as unknown as Store;
const s4 = () => renderToStaticMarkup(<S4 store={stub} s={{ ...freshState("p1"), stage: "s4", signedIn: true, walletFromClaim: true, walletAddr: "0x" + "ab".repeat(20) }} />);

describe("S4 wallet step — the wallet is a key on this device", () => {
  it("never calls it an ERC-4337 smart wallet or a passkey validator", () => {
    const html = s4();
    expect(html).not.toMatch(/ERC-4337/);
    expect(html).not.toMatch(/passkey validator/i);
    expect(html).not.toMatch(/smart wallet/i);
    expect(html).not.toMatch(/WebAuthn/);
  });

  it("uses #399's wording for the key", () => {
    const html = s4();
    expect(html).toContain("Wallet key on this device");
    // Since #545 (core#425) the key and its signing rule are two rows, with the link state on the key row.
    expect(html).toContain("EVM signing key · sealed in the custody vault · ");
    expect(html).toContain("Only through the signature ceremony, after you approve");
  });

  it("makes no claims about lazy deployment, sponsored gas, guardians or an optional local key", () => {
    const html = s4();
    expect(html).not.toMatch(/deploys lazily|gas sponsored|Guardian module|Optional signing key/);
  });
});

// Tripwire: the onboarding source carries none of these phrases outside comments (so #399's
// contractDeltas tripwire can drop its Onboarding exception).
describe("tripwire — onboarding wallet copy", () => {
  it("Onboarding.tsx has no 'passkey validator', 'ERC-4337' or 'smart wallet' outside comments", () => {
    const hits = readFileSync("src/onboarding/Onboarding.tsx", "utf8")
      .split("\n")
      .map((l, i) => [l, i + 1] as const)
      .filter(([l]) => !/^\s*(\/\/|\/?\*|\{\/\*)/.test(l) && /passkey validator|ERC-4337|smart wallet/i.test(l))
      .map(([, n]) => n);
    expect(hits).toEqual([]);
  });
});
