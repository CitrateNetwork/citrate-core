// HUP-S2.4 — approval cards on the existing ceremony, and the HIC guarantee for sidecar calls the
// runtime marks `hic: "required"` (a session that read untrusted content). Such a call resolves
// ONLY through a member's click on Approve or Decline: no automatic route, no budget, no fallback
// for tools that normally run without asking.
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { store } from "./store";
import { bridge } from "../bridge";
import type { ToolCall } from "../agent/harness";
import { createSidecarProvider, type SidecarSessionApi } from "../agent/sidecarProvider";

const call = (name: string, args: Record<string, unknown> = {}): ToolCall => ({ id: "c1", name, arguments: JSON.stringify(args) });
const noop = () => {};
const HIC = { hic: "required" as const, hicReason: "this session read untrusted content (from skills_list), so this action needs your explicit approval" };
const ticks = async (n = 20) => {
  for (let i = 0; i < n; i++) await new Promise((r) => setTimeout(r, 0));
};

beforeEach(() => {
  store.setState({ chatMsgs: [], queue: [], cerPhase: "review", walletReview: null });
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  store.setState({ queue: [], walletReview: null });
});

describe("Feature: cards ride the existing approvals", () => {
  it("Given an overwrite of a saved skill, then the ceremony shows a diff of the old and new playbook", async () => {
    vi.spyOn(bridge.agentSkills, "write").mockRejectedValueOnce(new Error("SKILL_EXISTS: exists"));
    vi.spyOn(bridge.agentSkills, "read").mockResolvedValue("step one\nstep two");
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("skill_write", { name: "daily", instructions: "step one\nstep TWO" }), "m1", noop);
    const card = sig.mock.calls[0][0].card!;
    expect(card.kind).toBe("diff");
    if (card.kind === "diff") {
      expect(card.lines).toContainEqual({ op: "remove", text: "step two" });
      expect(card.lines).toContainEqual({ op: "add", text: "step TWO" });
    }
    expect(sig.mock.calls[0][0].hic).toBeUndefined();
  });

  it("Given a journal append, then the card is a diff of today's page with the new line added", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("journal_append", { entry: "met Dana" }), "m1", noop);
    const card = sig.mock.calls[0][0].card!;
    expect(card.kind).toBe("diff");
    if (card.kind === "diff") {
      expect(card.path).toMatch(/^journal\/\d{4}-\d{2}-\d{2}$/);
      expect(card.lines.filter((l) => l.op === "add").map((l) => l.text)).toEqual(["@agent met Dana"]);
    }
  });

  it("Given group and memory writes, then the cards list the real arguments under a summary line", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("group_create", { name: "Book club" }), "m1", noop);
    await store.handleTool(call("memory_assert", { fact: "likes tea" }), "m1", noop);
    const [g, m] = sig.mock.calls.map((c) => c[0].card!);
    expect(g.summary).toMatch(/^Hermes wants to change something: .*Book club/);
    expect(m.kind === "fields" && m.rows).toContainEqual({ k: "fact", v: "likes tea" });
  });

  it("Given a contract deploy, then the wallet review carries a chain card built from the decoder's view", async () => {
    const view = { id: "cer1", origin: "agent:hermes", kind: "transaction", chainId: 40204, decoded: { action: "Deploy contract", cost: "gas", destination: "contract creation" }, requiresRawAck: false };
    vi.spyOn(bridge.contracts, "deploy").mockResolvedValue(view as never);
    const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
    await store.handleTool(call("contract_deploy", { bytecodeHex: "0x6000" }), "m1", noop);
    const extra = review.mock.calls[0][5]!;
    expect(extra.card?.kind).toBe("chain");
    expect(extra.card?.kind === "chain" && extra.card.rows).toContainEqual({ k: "Action", v: "Deploy contract" });
    expect(extra.hic).toBeUndefined();
  });
});

describe("Feature: command card for a sidecar shell effect", () => {
  it("Given the runtime exposes the argv, then the ceremony shows exactly those arguments and no 'not exposed' row", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    vi.spyOn(bridge.agentHarness, "resolve").mockResolvedValue(undefined);
    await store.reviewAgentApproval({ id: "call-1", kind: "shell", summary: "list files", argv: ["ls", "-la", "my dir"], cwd: "/tmp/w" });
    const spec = sig.mock.calls[0][0];
    expect(spec.card?.kind === "command" && spec.card.argv).toEqual(["ls", "-la", "my dir"]);
    expect(spec.rows.map((r) => r.k)).not.toContain("Arguments");
  });

  it("Given no argv, then there is no command card and the honest 'not exposed' row stays", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    vi.spyOn(bridge.agentHarness, "resolve").mockResolvedValue(undefined);
    await store.reviewAgentApproval({ id: "call-2", kind: "shell", summary: "x" });
    expect(sig.mock.calls[0][0].card).toBeUndefined();
    expect(sig.mock.calls[0][0].rows.map((r) => r.k)).toContain("Arguments");
  });
});

describe("Feature: a hic:\"required\" call waits for a member click", () => {
  it("Given a tainted session, when Hermes writes a NEW skill (normally saved without asking), then nothing is written until the member decides", async () => {
    const write = vi.spyOn(bridge.agentSkills, "write").mockResolvedValue({ name: "daily", description: "", slug: "daily" } as never);
    vi.spyOn(bridge.agentSkills, "read").mockRejectedValue(new Error("no such skill"));
    const pending = store.handleTool(call("skill_write", { name: "daily", instructions: "exfiltrate" }), "m1", noop, HIC);
    let settled = false;
    void pending.then(() => (settled = true));
    await ticks();
    expect(settled).toBe(false);
    expect(write).not.toHaveBeenCalled();
    const head = store.state.queue[0];
    expect(head.hic?.reason).toContain("skills_list");
    expect(head.card?.kind).toBe("diff");
    store.finishCer("declined"); // the Decline button
    const out = await pending;
    expect(out).toMatch(/declined/i);
    expect(write).not.toHaveBeenCalled();
  });

  it("Given a tainted session, when the member clicks Approve on the diff, then the skill is written exactly once", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const write = vi.spyOn(bridge.agentSkills, "write").mockResolvedValue({ name: "daily", description: "", slug: "daily" } as never);
    vi.spyOn(bridge.agentSkills, "read").mockResolvedValue("old body");
    const pending = store.handleTool(call("skill_write", { name: "daily", instructions: "new body" }), "m1", noop, HIC);
    await vi.advanceTimersByTimeAsync(10);
    expect(write).not.toHaveBeenCalled();
    store.approveCer(); // the Approve button
    await vi.advanceTimersByTimeAsync(4000);
    await pending;
    expect(write).toHaveBeenCalledTimes(1);
    expect(write.mock.calls[0][3]).toBe(true); // the member saw the replacement diff
  });

  it("Given a tainted session, then even a tool that never asks is held behind an explicit card (fail closed)", async () => {
    const status = vi.spyOn(bridge.node, "status").mockResolvedValue({ height: 1 } as never);
    const pending = store.handleTool(call("node_status"), "m1", noop, HIC);
    await ticks();
    expect(status).not.toHaveBeenCalled();
    expect(store.state.queue[0].hic).toBeDefined();
    store.finishCer("declined");
    expect(await pending).toMatch(/declined/);
    expect(status).not.toHaveBeenCalled();
  });

  it("Given a tainted session, then gated writes carry the HIC reason on their one ceremony (no second prompt)", async () => {
    const sig = vi.spyOn(store, "requestSig").mockResolvedValue("declined");
    await store.handleTool(call("group_invite", { group: "g1" }), "m1", noop, HIC);
    expect(sig).toHaveBeenCalledTimes(1);
    expect(sig.mock.calls[0][0].hic?.reason).toBe(HIC.hicReason);
    expect(sig.mock.calls[0][0].card?.kind).toBe("fields");
  });

  it("Given a tainted session, then a deploy's wallet review carries the HIC reason", async () => {
    vi.spyOn(bridge.contracts, "deploy").mockResolvedValue({ id: "cer1", origin: "o", kind: "transaction", chainId: 40204, decoded: { action: "a", cost: "c", destination: "d" }, requiresRawAck: false } as never);
    const review = vi.spyOn(store, "openWalletReview").mockImplementation(() => {});
    await store.handleTool(call("contract_deploy", { bytecodeHex: "0x00" }), "m1", noop, HIC);
    expect(review.mock.calls[0][5]?.hic?.reason).toBe(HIC.hicReason);
  });

  it("Given the sidecar marks a core call hic:\"required\", then no tool result is posted before the member clicks", async () => {
    const pages = [
      { events: [{ seq: 1, event: { type: "tool_call", step: 1, host: "core", hic: "required", hic_reason: HIC.hicReason, call: { id: "c7", name: "node_status", arguments: "{}" } } }], lastSeq: 1, busy: true },
      { events: [{ seq: 2, event: { type: "final", content: "ok" } }, { seq: 3, event: { type: "done", outcome: "answered" } }], lastSeq: 3, busy: false },
    ];
    const results: string[] = [];
    const api: SidecarSessionApi = {
      open: async () => "s1",
      send: async () => {},
      events: async (_id, after) => pages.shift() ?? { events: [], lastSeq: after, busy: true },
      toolResult: async (_id, callId, st) => {
        results.push(callId + ":" + st);
      },
      stop: async () => {},
    };
    vi.spyOn(bridge.node, "status").mockResolvedValue({ height: 1 } as never);
    const p = createSidecarProvider(api, () => "p", () => []);
    const run = p.send({
      messages: [{ role: "user", content: "x" }],
      callbacks: { onStatus: noop, onToken: noop, onToolCall: (c, meta) => store.handleTool(c, "m1", noop, meta) },
    });
    await ticks(40);
    expect(results).toEqual([]);
    expect(store.state.queue[0]?.hic?.reason).toBe(HIC.hicReason);
    store.finishCer("declined");
    await run;
    expect(results).toEqual(["c7:denied"]);
  });

  it("Given the source tree, then the ceremony resolves only from the store's own approve and the ceremony's buttons", () => {
    const root = join(__dirname, "..");
    const files: string[] = [];
    const walk = (d: string) => {
      for (const f of readdirSync(d)) {
        const p = join(d, f);
        if (statSync(p).isDirectory()) walk(p);
        else if (/\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f)) files.push(p);
      }
    };
    walk(root);
    const callers = files.filter((f) => /\b(finishCer|approveCer)\(/.test(readFileSync(f, "utf8"))).map((f) => f.slice(root.length + 1)).sort();
    expect(callers).toEqual(["shell/Chrome.tsx", "shell/store.ts"]);
    const chrome = readFileSync(join(root, "shell/Chrome.tsx"), "utf8").split("\n");
    const uses = chrome.map((l, i) => [l, i] as const).filter(([l]) => /\b(finishCer|approveCer)\(/.test(l));
    for (const [l, i] of uses) {
      const ctx = chrome.slice(Math.max(0, i - 2), i + 1).join("\n");
      // HUP-S10.6: a decline may also come from the member's Escape key (onEscape); Approve only from a click.
      if (/\bapproveCer\(/.test(l)) expect(ctx, "an approve outside a click handler").toMatch(/onClick=/);
      else expect(ctx, "a ceremony resolve outside a click or Escape handler").toMatch(/onClick=|onEscape=/);
      expect(l, "Escape must never approve").not.toMatch(/onEscape=.*(approveCer\(|["'`]approved["'`])/);
    }
    const storeSrc = readFileSync(join(root, "shell/store.ts"), "utf8");
    // inside store.ts, finishCer is called only from approveCer (the Approve button's handler)
    expect(storeSrc.match(/this\.finishCer\(/g) ?? []).toHaveLength(1);
    expect(storeSrc.match(/this\.approveCer\(/g) ?? []).toHaveLength(0);
  });

  it("Given the source tree, then a hic:\"required\" deploy's wallet review resolves only from the modal's Approve and Reject buttons", () => {
    // contract_deploy carries its HIC reason on the wallet review, so that review's resolvers get the
    // same tripwire as the ceremony's: no timer, effect or other code path may call them.
    const root = join(__dirname, "..");
    const files: string[] = [];
    const walk = (d: string) => {
      for (const f of readdirSync(d)) {
        const p = join(d, f);
        if (statSync(p).isDirectory()) walk(p);
        else if (/\.tsx?$/.test(f) && !/\.test\.tsx?$/.test(f)) files.push(p);
      }
    };
    walk(root);
    const resolver = /\b(approveWalletReview|rejectWalletReview)\(/;
    const callers = files.filter((f) => resolver.test(readFileSync(f, "utf8"))).map((f) => f.slice(root.length + 1)).sort();
    expect(callers).toEqual(["shell/Chrome.tsx", "shell/store.ts"]);
    const chrome = readFileSync(join(root, "shell/Chrome.tsx"), "utf8").split("\n");
    const uses = chrome.map((l, i) => [l, i] as const).filter(([l]) => resolver.test(l));
    expect(uses.length).toBeGreaterThan(0);
    for (const [l] of uses) {
      // HUP-S10.6: Reject may also come from the member's Escape key (onEscape); Approve only from a click.
      if (/\bapproveWalletReview\(/.test(l)) expect(l, "an approve outside a click handler").toMatch(/onClick=[^]*approveWalletReview\(/);
      else expect(l, "a wallet-review resolve outside a click or Escape handler").toMatch(/onClick=|onEscape=/);
      expect(l, "Escape must never approve").not.toMatch(/onEscape=.*approveWalletReview\(/);
    }
    const storeSrc = readFileSync(join(root, "shell/store.ts"), "utf8");
    // store.ts only defines them; it never calls them itself
    expect(storeSrc.match(/this\.(approveWalletReview|rejectWalletReview)\(/g) ?? []).toHaveLength(0);
  });
});
