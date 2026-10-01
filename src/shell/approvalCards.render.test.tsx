// HUP-S2.4 — the existing approval UI renders the annotation cards: the SignatureCeremony shows a
// diff, the exact argv, or named fields under one summary line; the wallet review adds the chain
// card's summary above the decoder's own rows; a hic:"required" request shows why it needs the
// member's explicit decision. Static renders (no clicks fire).
import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { SignatureCeremony, WalletReviewModal } from "./Chrome";
import { freshState, type AppState, type CerSpec } from "./state";
import type { Store } from "./store";
import { diffCard, commandCard, fieldsCard, chainCard } from "../agent/approvalCards";
import type { CeremonyView } from "../bridge/types";

const noopStore = {} as unknown as Store;
const write = { effect: "write", trust: "trusted" } as const;
const base: CerSpec = { origin: "chat agent", requester: "dashboard agent · tool x", title: "T", rows: [], cost: "none", sponsor: "no chain transaction", sponsorColor: "var(--tx-3)", chainless: true };

function withHead(head: CerSpec): AppState {
  const s = freshState("p1");
  s.queue = [head];
  s.cerPhase = "review";
  return s;
}

describe("Feature: the SignatureCeremony renders approval cards", () => {
  it("Given a diff card, then added and removed lines are marked and the summary leads", () => {
    const html = renderToStaticMarkup(<SignatureCeremony store={noopStore} s={withHead({ ...base, card: diffCard("skill_write", write, "skills/daily.md", "keep\nold", "keep\nnew") })} />);
    expect(html).toContain('data-testid="card-diff"');
    expect(html).toContain("skills/daily.md");
    expect(html).toMatch(/data-op="remove"[^>]*>- old/);
    expect(html).toMatch(/data-op="add"[^>]*>\+ new/);
    expect(html.indexOf('data-testid="card-summary"')).toBeLessThan(html.indexOf('data-op="remove"'));
  });

  it("Given a command card, then every argument is listed separately and quoted exactly", () => {
    const html = renderToStaticMarkup(<SignatureCeremony store={noopStore} s={withHead({ ...base, card: commandCard("run", write, ["rm", "-rf", "a b"], "/w") })} />);
    const args = [...html.matchAll(/data-testid="argv"[^>]*>([^<]*)</g)].map((m) => m[1]);
    expect(args).toEqual(["&quot;rm&quot;", "&quot;-rf&quot;", "&quot;a b&quot;"]);
    expect(html).toContain("in /w");
  });

  it("Given a fields card, then the arguments are shown by name", () => {
    const html = renderToStaticMarkup(<SignatureCeremony store={noopStore} s={withHead({ ...base, card: fieldsCard("group_create", write, { name: "Book club" }, "create the group") })} />);
    expect(html).toContain('data-testid="card-fields"');
    expect(html).toContain("Book club");
    expect(html).toContain("Hermes wants to change something: create the group");
  });

  it("Given a hic:\"required\" request, then the banner gives the reason and both decision buttons are present", () => {
    const html = renderToStaticMarkup(<SignatureCeremony store={noopStore} s={withHead({ ...base, hic: { reason: "this session read untrusted content (from skills_list)" }, card: fieldsCard("node_status", null, {}, "call node_status") })} />);
    expect(html).toContain('data-testid="hic-banner"');
    expect(html).toContain("from skills_list");
    expect(html).toMatch(/no automatic approval/i);
    expect(html).toContain("Decline");
    expect(html).toContain("Approve");
  });

  it("Given an ordinary request, then no HIC banner is shown", () => {
    const html = renderToStaticMarkup(<SignatureCeremony store={noopStore} s={withHead({ ...base, card: fieldsCard("group_create", write, {}, "x") })} />);
    expect(html).not.toContain('data-testid="hic-banner"');
  });
});

describe("Feature: the wallet review carries the chain card", () => {
  const view: CeremonyView = { id: "cer-1", origin: "agent:hermes", kind: "transaction", chainId: 40204, decoded: { action: "Deploy contract (12 bytes)", destination: "contract creation", cost: "gas" }, requiresRawAck: false };
  it("Given a deploy with a chain card and a HIC reason, then the summary and banner sit above the decoder's rows", () => {
    const s = freshState("p1");
    s.walletReview = { kind: "deploy", label: "Deploy contract", view, rawAck: false, card: chainCard("contract_deploy", { effect: "sign", trust: "trusted" }, view), hic: { reason: "tainted" } };
    const html = renderToStaticMarkup(<WalletReviewModal store={noopStore} s={s} />);
    expect(html).toContain('data-testid="hic-banner"');
    expect(html).toContain("Hermes wants your signature: Deploy contract (12 bytes) on chain 40204");
    // the decoded rows appear once (the modal's own), not duplicated by the card
    expect(html.split("contract creation").length - 1).toBe(1);
  });
});
