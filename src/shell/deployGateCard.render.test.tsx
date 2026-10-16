// HUP-S6.4 — the deploy gate verdict card, standalone and on the wallet review (Signature
// Ceremony) for a contract deploy. Static renders (no clicks fire).
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { DeployGateCard } from "./DeployGateCard";
import { WalletReviewModal } from "./Chrome";
import { freshState } from "./state";
import type { Store } from "./store";
import { GATE_ITEM_IDS, type DeployGateRecord } from "../agent/deployGate";
import type { CeremonyView } from "../bridge/types";

const H = "0xaa5f2337da9c808fb2ad5e1f956315f065d21e8369c061776c3bb6933ce0a1c6";
const LABEL = { forge_tests: "Forge tests", slither: "Slither", aderyn: "Aderyn", medusa: "Medusa campaign", fork_dry_run: "Fork dry run" } as const;

function rec(failing: Partial<Record<(typeof GATE_ITEM_IDS)[number], string>> = {}): DeployGateRecord {
  const items = GATE_ITEM_IDS.map((id) => ({
    id,
    label: LABEL[id],
    pass: !(id in failing),
    reason: failing[id] ?? "ok",
    evidence: { counts: { high: 0 }, outputSha256: "ab".repeat(32), durationMs: 1000, toolVersion: null },
  }));
  return {
    initcodeHash: H,
    bindingHash: "0x" + "cd".repeat(32),
    compiler: { solcVersion: "0.8.28", optimizer: true, optimizerRuns: 200, evmVersion: "cancun", viaIr: false },
    verdict: Object.keys(failing).length ? "NOT_READY" : "READY",
    items,
    evaluatedAtMs: 1,
  };
}

describe("Feature: the deploy gate verdict card", () => {
  it("Given a READY gate, then the card shows READY, the bytecode hash and every item", () => {
    const html = renderToStaticMarkup(<DeployGateCard record={rec()} initcodeHash={H} />);
    expect(html).toContain('data-testid="deploy-gate-card"');
    expect(html).toMatch(/data-testid="deploy-gate-verdict"[^>]*>READY</);
    expect(html).toContain(H);
    for (const l of Object.values(LABEL)) expect(html).toContain(l);
    expect(html).not.toContain("NOT READY");
  });

  it("Given a NOT READY gate, then the card shows NOT READY with the failing reasons", () => {
    const html = renderToStaticMarkup(<DeployGateCard record={rec({ aderyn: "aderyn is not installed (a missing tool is a fail, never a pass)" })} initcodeHash={H} />);
    expect(html).toMatch(/data-testid="deploy-gate-verdict"[^>]*>NOT READY</);
    expect(html).toMatch(/data-testid="deploy-gate-reason"[^>]*>Aderyn: aderyn is not installed/);
  });

  it("Given no gate record, then the card shows NOT READY and the hash it looked up", () => {
    const html = renderToStaticMarkup(<DeployGateCard record={null} initcodeHash={H} />);
    expect(html).toMatch(/NOT READY/);
    expect(html).toContain(H);
    expect(html).toMatch(/No deploy gate has run for this exact bytecode/);
  });
});

describe("Feature: the Signature Ceremony shows the gate verdict for a deploy", () => {
  it("Given a deploy review carrying its gate record, then the review shows the verdict and the hash", () => {
    const view: CeremonyView = { id: "cer1", origin: "local-user", kind: "transaction" as CeremonyView["kind"], chainId: 40204, decoded: { action: "contract creation", cost: "", destination: "" }, requiresRawAck: false };
    const s = freshState("p1");
    s.walletReview = { kind: "deploy", label: "Deploy contract", view, rawAck: false, deployGate: rec() };
    const html = renderToStaticMarkup(<WalletReviewModal store={{} as unknown as Store} s={s} />);
    expect(html).toContain('data-testid="deploy-gate-card"');
    expect(html).toMatch(/data-testid="deploy-gate-verdict"[^>]*>READY</);
    expect(html).toContain(H);
  });

  it("Given a review without a gate record, then no gate card is shown (non-deploy reviews unchanged)", () => {
    const view: CeremonyView = { id: "cer2", origin: "local-user", kind: "transaction" as CeremonyView["kind"], chainId: 40204, decoded: { action: "Send", cost: "1 SALT", destination: "0xabc" }, requiresRawAck: false };
    const s = freshState("p1");
    s.walletReview = { kind: "send", label: "Send SALT", view, rawAck: false };
    const html = renderToStaticMarkup(<WalletReviewModal store={{} as unknown as Store} s={s} />);
    expect(html).not.toContain("deploy-gate-card");
  });
});
