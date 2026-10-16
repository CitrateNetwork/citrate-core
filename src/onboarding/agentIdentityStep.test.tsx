// HUP-S7.4 (US-7.1) — the onboarding "give Hermes an identity" step. Static render of the
// pure view: disabled with the honest reason on today's chain, enabled only when core says so.
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { AgentIdentityView } from "./AgentIdentityStep";
import { identityCard, type AgentSbtStatus } from "../identity/agentSbt";

const status = (over: Partial<AgentSbtStatus> = {}): AgentSbtStatus => ({
  contract: "0xd16b1ad6e744f3e92223c65f492c35d36ae07c7b",
  member: "0x1111111111111111111111111111111111111111",
  did: "did:citrate:agent:0x1111111111111111111111111111111111111111",
  parentOrgId: "0",
  balance: "0",
  tokens: [],
  tokensNote: null,
  state: "org-not-active",
  available: false,
  message: "Hermes identity is available after the network upgrade. The member organization that Hermes identities belong to is not active on chain yet.",
  ...over,
});
const tauri = { mode: "tauri" as const, loaded: true, error: null };
const noop = () => {};

describe("AgentIdentityView", () => {
  it("today's chain: the button is disabled and the reason is shown", () => {
    const html = renderToStaticMarkup(<AgentIdentityView card={identityCard(status(), tauri)} busy={false} onMint={noop} />);
    expect(html).toContain('data-testid="agent-identity-step"');
    expect(html).toContain("available after the network upgrade");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*data-testid="agent-identity-mint"|<button[^>]*data-testid="agent-identity-mint"[^>]*disabled=""/);
  });

  it("ready: the button is enabled and the ceremony is named", () => {
    const card = identityCard(
      status({ state: "ready", available: true, message: "You approve one transaction in the Signature Ceremony; it costs only network gas." }),
      tauri,
    );
    const html = renderToStaticMarkup(<AgentIdentityView card={card} busy={false} onMint={noop} />);
    expect(html).toContain("Signature Ceremony");
    expect(html).not.toMatch(/data-testid="agent-identity-mint"[^>]*disabled/);
    expect(html).toContain("Give Hermes an identity");
  });

  it("busy disables the button while the ceremony is prepared", () => {
    const card = identityCard(status({ state: "ready", available: true }), tauri);
    const html = renderToStaticMarkup(<AgentIdentityView card={card} busy onMint={noop} />);
    expect(html).toMatch(/disabled=""/);
    expect(html).toContain("Preparing");
  });

  it("minted: the token is listed and no mint button is shown", () => {
    const card = identityCard(
      status({
        state: "minted",
        balance: "1",
        message: "Hermes has an on-chain identity (AgentSBT) bound to your wallet.",
        tokens: [{ tokenId: "4", parentOrgId: "0", did: "0x8aca81cc" + "0".repeat(56), pubkeyFingerprint: "0x21fe" + "0".repeat(60), quarantined: false }],
      }),
      tauri,
    );
    const html = renderToStaticMarkup(<AgentIdentityView card={card} busy={false} onMint={noop} />);
    expect(html).toContain("AgentSBT #4");
    expect(html).not.toContain('data-testid="agent-identity-mint"');
  });

  it("names the data source, including the membership SBT and the member mint", () => {
    const html = renderToStaticMarkup(<AgentIdentityView card={identityCard(status(), tauri)} busy={false} onMint={noop} />);
    expect(html).toContain("AgentSBT on chain 40204");
    expect(html).toContain("CitrateMemberSBT");
    expect(html).toContain("mintAgentAsMember");
  });

  it("says the member mints from their own wallet", () => {
    const card = identityCard(status({ state: "ready", available: true, message: "x" }), tauri);
    const html = renderToStaticMarkup(<AgentIdentityView card={card} busy={false} onMint={noop} />);
    expect(html).toContain("from your own wallet");
  });
});
