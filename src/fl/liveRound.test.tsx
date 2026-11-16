// FL design board 06, Rounds › Live round: every round state (pending, running, verifying,
// accepted, rejected, disputed, expired, coordinator silent) and the StateMatrix columns. The
// rounds here are TEST-ONLY fixtures; no production module builds a LiveRound in 0.5.4.
import { describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { renderToStaticMarkup } from "react-dom/server";
import { LiveRoundView, NEXT, NoLiveRound, ringLabel, stateLine, type LiveRound, type LiveState } from "./LiveRound";
import type { Lease } from "./LeaseDots";

const SALT = 10n ** 18n;
const leases = (codes: string): Lease[] =>
  codes.split("").map((c, i) => ({
    id: String(i + 1),
    mine: i === 0,
    state: ({ L: "leased", S: "submitted", V: "verified", X: "rejected" } as const)[c as "L"],
    reason: c === "X" ? "the model update failed the file format check" : undefined,
  }));

const base = (state: LiveState, over: Partial<LiveRound> = {}): LiveRound => ({
  offerId: 14,
  poster: "Orchard Lab",
  state,
  coordinatorAnswering: true,
  leases: leases("VVVVVVXS"),
  maxWorkers: 8,
  clock: { state: "open", commitBlock: 1_204_311, endBlock: 1_209_911, head: 1_206_611, blockSeconds: 2 },
  belnap: { t: 6, f: 1, both: 0, neither: 1 },
  escrow: { budgetWei: 600n * SALT, paidWei: 0n, reservedWei: 9n * SALT, refundableWei: 591n * SALT },
  contribution: [
    { key: "dataset", state: "done", detail: "support-tickets v3" },
    { key: "training", state: "done", detail: "1 h 42 min" },
  ],
  rateWei: (3n * SALT) / 2n,
  deadline: 1_763_769_600,
  payouts: "dark",
  ids: { frlRound: "0x" + "3e".repeat(32), mergedHash: "0x" + "a7".repeat(32), coordinator: "https://dana-node.example:7443" },
  ...over,
});

const html = (r: LiveRound) => renderToStaticMarkup(<LiveRoundView r={r} go={() => {}} />);
const text = (h: string) => {
  const el = document.createElement("div");
  el.innerHTML = h;
  return el.textContent ?? "";
};

describe("Live round: all eight states", () => {
  it("pending: waiting for leases, ring on Open, clock not open, Belnap —", () => {
    const h = html(base("pending", { leases: leases("LLL"), clock: { state: "not-open", head: null, blockSeconds: null }, belnap: null, escrow: { budgetWei: 600n * SALT, paidWei: 0n, reservedWei: 0n, refundableWei: 600n * SALT } }));
    expect(text(h)).toContain("Waiting for leases · 3 of 8 taken");
    expect(h).toContain(`data-stage="open"`);
    expect(text(h)).toContain("opens when the merge is committed");
    expect((text(h).match(/—/g) ?? []).length).toBeGreaterThanOrEqual(4);
    expect(text(h)).toContain(NEXT.pending);
  });
  it("running: leases out and submitted", () => {
    const h = html(base("running", { leases: leases("SSLLLLL"), clock: { state: "not-open", head: null, blockSeconds: null }, belnap: null }));
    expect(text(h)).toContain("7 leases out · 2 submitted");
    expect(h).toContain(`data-stage="running"`);
  });
  it("verifying: merge committed, window open, the measured duration, the escrow split", () => {
    const t = text(html(base("verifying")));
    expect(t).toContain("Merge committed at block 1,204,311 · window open");
    expect(t).toContain("Challenge window ends at block 1,209,911, in about 1 h 50 min");
    expect(t).toContain("Escrow of 600.00 SALT: 0.00 paid, 9.00 reserved, 591.00 refundable");
    expect(t).toContain("Belnap result");
  });
  it("accepted: Accepted on chain at block N, never final or settled; Check the merged adapter", () => {
    const h = html(base("accepted", { acceptedBlock: 1_209_911, clock: { state: "closed", commitBlock: 1, endBlock: 1_209_911, head: 1_210_000, blockSeconds: 2 } }));
    const t = text(h);
    expect(t).toContain("Accepted on chain at block 1,209,911");
    expect(t).not.toMatch(/\b(final|settled)\b/i);
    expect(t).toContain("Check the merged adapter");
    expect(h).toContain(`data-stage="payout"`);
  });
  it("rejected: the fault, never pays, the ring marks Verifying rejected", () => {
    const h = html(base("rejected", { fault: "output mismatch", clock: { state: "ended-early", head: 1, blockSeconds: null } }));
    expect(text(h)).toContain("Rejected: output mismatch. A challenger replayed the merge and it did not match. A rejected round never pays");
    expect(h).toContain(`data-seg="verifying" data-paint="rejected"`);
    expect(text(h)).toContain("a challenge was proven");
  });
  it("disputed: payout frozen, the ring marks Payout disputed", () => {
    const h = html(base("disputed", { payouts: "open" }));
    expect(text(h)).toContain("A worker disputed the payout list · payout frozen");
    expect(text(h)).toContain("Frozen while the dispute is open.");
    expect(h).toContain(`data-seg="payout" data-paint="disputed"`);
  });
  it("expired: full refund, no payout, every segment grey", () => {
    const h = html(base("expired", { leases: leases("LL"), belnap: null, clock: { state: "not-open", head: null, blockSeconds: null } }));
    expect(text(h)).toContain("Deadline passed before enough updates · full refund, no payout");
    expect([...h.matchAll(/data-paint="(\w+)"/g)].every((m) => m[1] === "expired")).toBe(true);
  });
  it("coordinator silent: the last report, on-chain state unaffected, your update stays here", () => {
    const t = text(html(base("running", { coordinatorAnswering: false, coordinatorLastSeenSecs: 240 })));
    expect(t).toContain("Coordinator not answering (last seen 4 min ago). Lease counts are the last coordinator report. On-chain state is unaffected.");
    expect(t).toContain("Your update stays on this device.");
  });
});

describe("Live round: StateMatrix columns", () => {
  it("Dark: the payout box says Payouts open after the security review", () => {
    expect(text(html(base("accepted", { payouts: "dark" })))).toContain("Payouts open after the security review");
  });
  it("live payouts show the settled line, SALT only, and offer a dispute", () => {
    const t = text(html(base("accepted", { payouts: "open", payout: { units: 7n, amountWei: (21n * SALT) / 2n } })));
    expect(t).toContain("7 units × 1.50 SALT = 10.50 SALT. Claim opens after the dispute window.");
    expect(t).toContain("My lease is missing: dispute");
    expect(t).not.toMatch(/USD|\$|earn/);
  });
  it("Unavailable: leases not reported and Belnap — are said, never zero", () => {
    const t = text(html(base("running", { leases: null, belnap: null })));
    expect(t).toContain("the coordinator has not reported leases");
    expect(ringLabel(base("running", { leases: null }))).toContain("leases not reported");
  });
  it("the ring's label is the full sentence", () => {
    expect(ringLabel(base("verifying"))).toBe("Round 14: verifying, 6 of 8 leases verified, challenge window ends at block 1,209,911, in about 1 h 50 min");
    expect(stateLine(base("accepted"))).toBe("Accepted on chain · payout list posted");
  });
  it("selecting a lease says it in words; ids sit behind a toggle", () => {
    const el = document.createElement("div");
    document.body.appendChild(el);
    const root = createRoot(el);
    act(() => root.render(<LiveRoundView r={base("verifying")} go={() => {}} />));
    expect(el.textContent).not.toContain("Merged hash");
    act(() => el.querySelector<HTMLButtonElement>('[data-lease="7"]')!.click());
    expect(el.textContent).toContain("Lease 7: rejected, the model update failed the file format check");
    act(() => [...el.querySelectorAll("button")].find((b) => b.textContent === "Show ids and hashes")!.click());
    expect(el.textContent).toContain("Merged hash");
    act(() => root.unmount());
    el.remove();
  });
  it("WorkMarket deployed, no round read model: no live round to show", () => {
    const go = vi.fn();
    const el = document.createElement("div");
    const root = createRoot(el);
    act(() => root.render(<NoLiveRound go={go} />));
    expect(el.textContent).toContain("No live round to show");
    act(() => el.querySelector("button")!.click());
    expect(go).toHaveBeenCalledWith("groups/rounds");
    act(() => root.unmount());
  });
});
