// 0.5.4 WP-H12: the federated learning part of Hermes's prompt is true of this build. A line names
// only tools this build has; every FL tool has guidance; the "no card" list is exactly the reviewed
// authority-lowering tools; and the not-live wording is the UI truth contract's.
import { describe, expect, it } from "vitest";
import realBook from "../../src-tauri/addresses/40204.json";
import { AGENT_SYSTEM_PROMPT, AGENT_TOOLS, AUTHORITY_LOWERING_AGENT_TOOLS } from "./harness";
import * as P from "./flPrompt";
import * as NS from "../fl/networkState";
import { PINNED, flSnapshot } from "../fl/flSnapshot";

const names = new Set(AGENT_TOOLS.map((t) => t.function.name as string));
const FL_FAMILY = /^(fl_|cluster_)|^group_(join|role|invite_revoke|invites)|^round_draft_propose$|^worker_terms_propose$/;

describe("the FL prompt names only tools this build has", () => {
  it("FL_TOOLS_IN_BUILD is exactly the guide's tools that AGENT_TOOLS offers (update it when a tool lands)", () => {
    const expected = [...P.FL_GUIDE_TOOLS].filter((t) => names.has(t)).sort();
    expect([...P.FL_TOOLS_IN_BUILD].sort()).toEqual(expected);
  });

  it("every FL tool in AGENT_TOOLS has a guide line", () => {
    const missing = [...names].filter((n) => FL_FAMILY.test(n) && !P.FL_GUIDE_TOOLS.has(n));
    expect(missing).toEqual([]);
  });

  it("the section mentions no guide tool that is not in this build", () => {
    for (const t of P.FL_GUIDE_TOOLS) {
      if (!P.FL_TOOLS_IN_BUILD.has(t)) expect(P.FL_PROMPT_SECTION, t).not.toMatch(new RegExp(`\\b${t}\\b`));
    }
    for (const t of P.FL_TOOLS_IN_BUILD) expect(P.FL_PROMPT_SECTION, t).toContain(t);
  });

  it("lists as no-card exactly the reviewed authority-lowering tools in this build", () => {
    const flLowering = [...AUTHORITY_LOWERING_AGENT_TOOLS].filter((t) => P.FL_GUIDE_TOOLS.has(t)).sort();
    expect(P.loweringTools().sort()).toEqual(flLowering);
    for (const t of P.loweringTools()) expect(AUTHORITY_LOWERING_AGENT_TOOLS.has(t), t).toBe(true);
    // 0.5.4 integration: HB's consent and invite tools landed with HC's adapters.
    expect(P.FL_PROMPT_SECTION).toMatch(/no card, because they only lower the member's exposure: fl_consent_withdraw, group_invite_revoke, fl_adapter_unload\./);
  });

  it("never lists a tool that can raise authority as no-card (envelope on is behind a card)", () => {
    const all = P.loweringTools(P.FL_GUIDE_TOOLS);
    expect(all).not.toContain("fl_envelope_propose");
    expect(all.sort()).toEqual(["fl_adapter_unload", "fl_consent_withdraw", "group_invite_revoke"]);
    expect(P.FL_GUIDE_TOOLS.has("fl_terms_propose")).toBe(false);
  });

  it("turns a lane's guidance on only with all of its tools", () => {
    // 0.5.4 integration: HB is in this build, so the "without HB" set is built by taking it out.
    const withoutHb = [...P.FL_TOOLS_IN_BUILD].filter((t) => !["fl_consent_list", "fl_consent_withdraw", "group_invites_list", "group_invite_revoke"].includes(t));
    const withHb = new Set([...withoutHb, "fl_consent_list", "fl_consent_withdraw", "group_invites_list", "group_invite_revoke"]);
    const text = P.flPromptLines(withHb).join("\n");
    expect(text).toContain("fl_consent_withdraw withdraws one at once");
    expect(text).toMatch(/only lower the member's exposure: fl_consent_withdraw, group_invite_revoke, fl_adapter_unload\./);
    expect(P.flPromptLines(new Set([...withoutHb, "fl_consent_list"])).join("\n")).not.toContain("fl_consent_list");
  });

  it("collection: fl_collect_* only once the collector ships, and an honest refusal before", () => {
    // 0.5.4 integration: HA's collector is in this build; the refusal is checked without it.
    const withoutHa = [...P.FL_TOOLS_IN_BUILD].filter((t) => t !== "fl_source_check" && t !== "fl_collect_plan");
    expect(P.flPromptLines(new Set(withoutHa)).join("\n")).toContain("Collecting web pages into a dataset is not in this build");
    expect(P.FL_PROMPT_SECTION).toContain("Use only fl_collect_* to build a dataset from the web");
    const withHa = P.flPromptLines(new Set([...withoutHa, "fl_source_check", "fl_collect_plan"])).join("\n");
    expect(withHa).toContain("Use only fl_collect_* to build a dataset from the web; never read_url, browser, shell or fs tools.");
    expect(withHa).not.toContain("not in this build: say so");
  });
});

describe("the FL prompt follows the truth contract and the copy rules", () => {
  it("uses networkState's exact not-live wording", () => {
    expect(P.ROUNDS_NOT_LIVE).toBe(NS.ROUNDS_NOT_LIVE);
    expect(P.NOT_AVAILABLE).toBe(NS.NOT_AVAILABLE);
    expect(P.PAYOUTS_DARK).toBe(NS.PAYOUTS_DARK);
    expect(P.NO_TRAINING_YET).toBe(NS.NO_TRAINING_YET);
  });

  it("is in the system prompt, and the old claims that were false are gone", () => {
    expect(AGENT_SYSTEM_PROMPT).toContain(P.FL_PROMPT_SECTION);
    expect(AGENT_SYSTEM_PROMPT).not.toMatch(/the one write that does not ask first/);
    expect(AGENT_SYSTEM_PROMPT).toMatch(/a change that only lowers the member's exposure/);
    // #742 review (P2): true once #716 puts every skill save behind a card.
    expect(AGENT_SYSTEM_PROMPT).toMatch(/Every skill save asks first/);
    expect(AGENT_SYSTEM_PROMPT).toMatch(/Never claim differential privacy or secure aggregation/);
    expect(AGENT_SYSTEM_PROMPT).toMatch(/You cannot sign, open a round, join, claim or publish/);
  });

  it("has no money words, no em dashes or exclamations, HIC not HITL, members not users", () => {
    const all = P.flPromptLines(P.FL_GUIDE_TOOLS).join("\n");
    expect(all).not.toContain("—");
    expect(all).not.toContain("!");
    expect(all).not.toMatch(/\bHITL\b|\bUSD\b|\$\d|\busers?\b/i);
    expect(all.replace(/never say earn/g, "")).not.toMatch(/\bearn/i);
  });
});

describe("the FL facts in Hermes's live snapshot", () => {
  it("say what the shipped book has (Today, 0.5.4)", () => {
    const s = flSnapshot(realBook, { label: "r1007" } as never);
    expect(s.contracts.WorkMarket).toBe(NS.NOT_AVAILABLE);
    expect(s.contracts.DatasetRegistry).toBe(NS.NOT_AVAILABLE);
    expect(s.contracts.FLRoundVerifier).toBe(NS.NOT_AVAILABLE);
    expect(s.contracts.FederatedRoundLedger).toBe(PINNED);
    expect(s.groupRounds).toBe(NS.ROUNDS_NOT_LIVE);
    expect(s.payouts).toContain(NS.NOT_AVAILABLE);
    expect(s.training).toBe(NS.NO_TRAINING_YET);
    // 0.5.4 integration: HB's envelope tools are in this build; without them the snapshot says HIC-1 only.
    expect(s.envelope).toBe("read it with fl_envelope_read; never state caps from memory");
    const noEnvelope = new Set([...P.FL_TOOLS_IN_BUILD].filter((t) => !t.startsWith("fl_envelope_")));
    expect(flSnapshot(realBook, { label: "r1007" } as never, noEnvelope).envelope).toMatch(/not in this build: every FL action is HIC-1/);
  });

  it("follow the book and the profile when a later network pins WorkMarket", () => {
    const book = { addresses: { ...realBook.addresses, WorkMarket: "0x" + "12".repeat(20) } };
    expect(flSnapshot(book, { label: "x" } as never).payouts).toBe(NS.PAYOUTS_DARK);
    expect(flSnapshot(book, { label: "x", payoutsOpen: true } as never).payouts).toBe("opened by the network profile; read them with a tool");
    expect(flSnapshot(book, null).groupRounds).toMatch(/WorkMarket is pinned/);
    expect(flSnapshot(book, null, new Set(["fl_envelope_read"])).envelope).toMatch(/fl_envelope_read/);
  });
});
