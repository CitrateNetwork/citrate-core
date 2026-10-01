// HUP-S10.6 — accessibility of the approval surfaces: the SignatureCeremony with every approval
// card kind and the HIC banner, the wallet review modal (decoded, raw calldata, deploy gate
// verdict) and the deploy gate verdict card on its own. axe-core over each state, then the
// keyboard contract: a named modal dialog, focus starts on the safe choice (Decline / Reject,
// never Approve), Tab and Shift+Tab stay inside, Escape declines while reviewing, a re-render
// does not steal focus, and focus returns to where it was when the dialog closes.
import { describe, it, expect, vi, afterEach } from "vitest";
import { SignatureCeremony, WalletReviewModal } from "../shell/Chrome";
import { DeployGateCard } from "../shell/DeployGateCard";
import { freshState, type AppState, type CerSpec } from "../shell/state";
import type { Store } from "../shell/store";
import { diffCard, commandCard, fieldsCard, chainCard } from "../agent/approvalCards";
import { GATE_ITEM_IDS, type DeployGateRecord } from "../agent/deployGate";
import type { CeremonyView } from "../bridge/types";
import { axeFindings, cleanupMounted, mount, press } from "./axeHarness";

afterEach(() => {
  cleanupMounted();
  document.body.innerHTML = "";
});

function fakeStore() {
  return {
    finishCer: vi.fn(),
    toast: vi.fn(),
    approveCer: vi.fn(),
    rejectWalletReview: vi.fn(async () => {}),
    approveWalletReview: vi.fn(async () => {}),
    setWalletReviewRawAck: vi.fn(),
  };
}
const asStore = (f: ReturnType<typeof fakeStore>) => f as unknown as Store;

const write = { effect: "write", trust: "trusted" } as const;
const base: CerSpec = { origin: "chat agent", requester: "dashboard agent · tool x", title: "Write a skill", rows: [{ k: "File", v: "skills/daily.md" }], cost: "none", sponsor: "no chain transaction", sponsorColor: "var(--tx-3)", chainless: true };

function cer(head: CerSpec, phase: AppState["cerPhase"] = "review", extra = 0): AppState {
  const s = freshState("p1");
  s.queue = [head, ...Array.from({ length: extra }, () => head)];
  s.cerPhase = phase;
  return s;
}

const H = "0xaa5f2337da9c808fb2ad5e1f956315f065d21e8369c061776c3bb6933ce0a1c6";
function gate(failing: string[] = []): DeployGateRecord {
  return {
    initcodeHash: H,
    bindingHash: "0x" + "cd".repeat(32),
    compiler: { solcVersion: "0.8.28", optimizer: true, optimizerRuns: 200, evmVersion: "cancun", viaIr: false },
    verdict: failing.length ? "NOT_READY" : "READY",
    items: GATE_ITEM_IDS.map((id) => ({
      id,
      label: id,
      pass: !failing.includes(id),
      reason: failing.includes(id) ? "1 high finding" : "ok",
      evidence: { counts: { high: failing.includes(id) ? 1 : 0 }, outputSha256: "ab".repeat(32), durationMs: 1000, toolVersion: null },
    })),
    evaluatedAtMs: 1,
  };
}
const view: CeremonyView = { id: "cer-1", origin: "agent:hermes", kind: "transaction", chainId: 40204, decoded: { action: "Deploy contract (12 bytes)", destination: "contract creation", cost: "gas" }, requiresRawAck: false };
function review(over: Partial<NonNullable<AppState["walletReview"]>> = {}): AppState {
  const s = freshState("p1");
  s.walletReview = { kind: "deploy", label: "Deploy contract", view, rawAck: false, ...over } as NonNullable<AppState["walletReview"]>;
  return s;
}

const ceremonyStates: [string, AppState][] = [
  ["diff card", cer({ ...base, card: diffCard("skill_write", write, "skills/daily.md", "keep\nold", "keep\nnew") })],
  ["command card", cer({ ...base, card: commandCard("run", write, ["rm", "-rf", "a b"], "/w") })],
  ["fields card + HIC banner", cer({ ...base, hic: { reason: "this session read untrusted content" }, card: fieldsCard("group_create", write, { name: "Book club" }, "create the group") })],
  ["warning + queue", cer({ ...base, warning: "This cannot be undone." }, "review", 2)],
  ["busy", cer(base, "busy")],
  ["done", cer(base, "done")],
];
const reviewStates: [string, AppState][] = [
  ["decoded + chain card + HIC", review({ card: chainCard("contract_deploy", { effect: "sign", trust: "trusted" }, view), hic: { reason: "tainted" } })],
  ["raw calldata", review({ view: { ...view, requiresRawAck: true } })],
  ["deploy gate NOT READY", review({ deployGate: gate(["slither"]) } as never)],
];

describe("HUP-S10.6 approval surfaces: axe-core", () => {
  for (const [name, s] of ceremonyStates) {
    it(`SignatureCeremony, ${name}: no axe violations`, async () => {
      const m = mount(<SignatureCeremony store={asStore(fakeStore())} s={s} />);
      expect(await axeFindings(m.host)).toEqual([]);
    });
  }
  for (const [name, s] of reviewStates) {
    it(`WalletReviewModal, ${name}: no axe violations`, async () => {
      const m = mount(<WalletReviewModal store={asStore(fakeStore())} s={s} />);
      expect(await axeFindings(m.host)).toEqual([]);
    });
  }
  for (const [name, r] of [["READY", gate()], ["NOT READY", gate(["slither", "medusa"])], ["no record", null]] as const) {
    it(`DeployGateCard ${name}: no axe violations`, async () => {
      const m = mount(<DeployGateCard record={r} initcodeHash={H} />);
      expect(await axeFindings(m.host)).toEqual([]);
    });
  }
});

describe("HUP-S10.6 SignatureCeremony: dialog semantics and keyboard", () => {
  it("is a modal dialog named by its title and described by the requester", () => {
    const m = mount(<SignatureCeremony store={asStore(fakeStore())} s={ceremonyStates[0][1]} />);
    const dlg = m.host.querySelector('[role="dialog"]');
    expect(dlg?.getAttribute("aria-modal")).toBe("true");
    expect(document.getElementById(dlg?.getAttribute("aria-labelledby") ?? "")?.textContent).toBe("Write a skill");
    expect(document.getElementById(dlg?.getAttribute("aria-describedby") ?? "")?.textContent).toMatch(/requested by/);
  });

  it("focus starts on Decline, never on Approve", () => {
    mount(<SignatureCeremony store={asStore(fakeStore())} s={ceremonyStates[0][1]} />);
    expect(document.activeElement?.textContent).toBe("Decline");
  });

  it("Tab from the last control wraps to the first; Shift+Tab from the first wraps to the last", () => {
    const m = mount(<SignatureCeremony store={asStore(fakeStore())} s={ceremonyStates[0][1]} />);
    const dlg = m.host.querySelector<HTMLElement>('[role="dialog"]');
    const focusables = [...(dlg?.querySelectorAll<HTMLElement>("button, [tabindex='0']") ?? [])];
    const first = focusables[0];
    const last = focusables[focusables.length - 1];
    last.focus();
    const ev = press("Tab");
    expect(ev.defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(first);
    press("Tab", { shift: true });
    expect(document.activeElement).toBe(last);
  });

  it("Tab never leaves the dialog for the page behind it", () => {
    const outside = document.createElement("button");
    outside.textContent = "behind";
    document.body.appendChild(outside);
    mount(<SignatureCeremony store={asStore(fakeStore())} s={ceremonyStates[0][1]} />);
    outside.focus();
    press("Tab", { target: outside });
    // focus that escaped is pulled back in on the next focusin
    outside.dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    expect(document.activeElement?.closest('[role="dialog"]')).not.toBeNull();
  });

  it("Escape while reviewing declines (nothing is signed); Escape while signing does nothing", () => {
    const f = fakeStore();
    mount(<SignatureCeremony store={asStore(f)} s={ceremonyStates[0][1]} />);
    press("Escape");
    expect(f.finishCer).toHaveBeenCalledWith("declined");
    expect(f.approveCer).not.toHaveBeenCalled();
    cleanupMounted();
    const g = fakeStore();
    mount(<SignatureCeremony store={asStore(g)} s={cer(base, "busy")} />);
    press("Escape");
    expect(g.finishCer).not.toHaveBeenCalled();
  });

  it("the diff is a named, keyboard-scrollable region", () => {
    const m = mount(<SignatureCeremony store={asStore(fakeStore())} s={ceremonyStates[0][1]} />);
    const region = m.host.querySelector<HTMLElement>('[data-testid="card-diff"] pre');
    expect(region?.getAttribute("tabindex")).toBe("0");
    expect(region?.getAttribute("aria-label")).toMatch(/skills\/daily\.md/);
  });

  it("signing progress is announced politely and the decorative marks are hidden", () => {
    const m = mount(<SignatureCeremony store={asStore(fakeStore())} s={cer(base, "busy")} />);
    const status = m.host.querySelector('[role="status"]');
    expect(status).not.toBeNull();
    for (const svg of m.host.querySelectorAll("svg")) expect(svg.closest('[aria-hidden="true"]') ?? svg.getAttribute("aria-hidden")).toBeTruthy();
  });

  it("focus returns to where it was when the ceremony closes", () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const m = mount(<SignatureCeremony store={asStore(fakeStore())} s={ceremonyStates[0][1]} />);
    expect(document.activeElement).not.toBe(opener);
    m.rerender(<SignatureCeremony store={asStore(fakeStore())} s={freshState("p1")} />);
    expect(document.activeElement).toBe(opener);
  });

  it("a re-render while reviewing does not pull focus back to Decline", () => {
    const f = fakeStore();
    const s = ceremonyStates[0][1];
    const m = mount(<SignatureCeremony store={asStore(f)} s={s} />);
    const approve = [...m.host.querySelectorAll("button")].find((b) => /Approve/.test(b.textContent ?? ""));
    approve?.focus();
    m.rerender(<SignatureCeremony store={asStore(f)} s={{ ...s }} />);
    expect(document.activeElement).toBe(approve);
  });
});

describe("HUP-S10.6 WalletReviewModal: keyboard", () => {
  it("focus starts on Reject, never on Approve", () => {
    mount(<WalletReviewModal store={asStore(fakeStore())} s={reviewStates[0][1]} />);
    expect(document.activeElement?.textContent).toBe("Reject");
  });

  it("is named by its heading", () => {
    const m = mount(<WalletReviewModal store={asStore(fakeStore())} s={reviewStates[0][1]} />);
    const dlg = m.host.querySelector('[role="dialog"]');
    expect(document.getElementById(dlg?.getAttribute("aria-labelledby") ?? "")?.textContent).toBe("You are about to sign");
  });

  it("ticking the raw-calldata acknowledgement keeps focus on the checkbox", () => {
    const f = fakeStore();
    const s = reviewStates[1][1];
    const m = mount(<WalletReviewModal store={asStore(f)} s={s} />);
    const box = m.host.querySelector<HTMLInputElement>('input[type="checkbox"]');
    box?.focus();
    const r = s.walletReview;
    if (!r) throw new Error("no review");
    m.rerender(<WalletReviewModal store={asStore(f)} s={{ ...s, walletReview: { ...r, rawAck: true } }} />);
    expect(document.activeElement).toBe(box);
  });

  it("Escape rejects; it never approves", () => {
    const f = fakeStore();
    mount(<WalletReviewModal store={asStore(f)} s={reviewStates[0][1]} />);
    press("Escape");
    expect(f.rejectWalletReview).toHaveBeenCalledTimes(1);
    expect(f.approveWalletReview).not.toHaveBeenCalled();
  });

  it("Tab wraps inside the dialog", () => {
    const m = mount(<WalletReviewModal store={asStore(fakeStore())} s={reviewStates[1][1]} />);
    const dlg = m.host.querySelector<HTMLElement>('[role="dialog"]');
    const focusables = [...(dlg?.querySelectorAll<HTMLElement>("button:not([disabled]), input, [tabindex='0']") ?? [])];
    focusables[focusables.length - 1].focus();
    press("Tab");
    expect(document.activeElement).toBe(focusables[0]);
  });
});

describe("HUP-S10.6 DeployGateCard: verdict is text and named", () => {
  it("is a group named with its verdict, and each item says pass or fail in words", () => {
    const m = mount(<DeployGateCard record={gate(["slither"])} initcodeHash={H} />);
    const card = m.host.querySelector('[data-testid="deploy-gate-card"]');
    expect(card?.getAttribute("role")).toBe("group");
    expect(card?.getAttribute("aria-label")).toMatch(/deploy gate: not ready/i);
    const items = [...m.host.querySelectorAll('[data-testid="deploy-gate-item"]')].map((i) => i.textContent ?? "");
    expect(items.some((t) => /^fail/.test(t))).toBe(true);
    expect(items.some((t) => /^pass/.test(t))).toBe(true);
  });
});
