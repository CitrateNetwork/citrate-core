// HUP-S3.4 (US-3.4) — the proposal card model.
// AC1: a proposal shows the verifier evidence. AC2: persisting requires the member's accept.
// AC3: publishing to the SkillRegistry is HIC-1 (and off until signed off). AC4: contradictions
// are surfaced and must be acknowledged, never silently merged.
import { describe, it, expect } from "vitest";
import { acknowledgedFor, isKnownRef, memoryRowModel, proposalCardModel, resolveChoice, setAsideChoice, type LearnedMemory, type LearnProposal } from "./learn";

const SKILL = "---\nname: deploy-checklist\ndescription: Checks a contract before deploy\n---\n\n1. Run the tests.\n";

function proposal(over: Partial<LearnProposal> = {}): LearnProposal {
  return {
    id: "lp-0123456789abcdef01234567",
    kind: "memory",
    content: { kind: "memory", key: "deploy chain", value: "40204" },
    content_sha256: "ab".repeat(32),
    evidence: {
      workflow_id: "check",
      steps: ["answer"],
      verdicts: [{ step: "answer", name: 'answer mentions "checks pass"', passed: true, detail: "" }],
      attempts: 1,
      trajectory: { session_id: "s1-ab", workflow_id: "check", messages: 2, sha256: "cd".repeat(32) },
    },
    provenance: { session_id: "s1-ab", agent: "hermes", model: "gemma-4" },
    created_at_ms: 1,
    conflicts: [],
    state: { state: "proposed" },
    ...over,
  };
}

const OFF = { enabled: false, note: "pending owner sign-off" };

describe("Feature: a proposal shows its evidence (AC1)", () => {
  it("Given a verified run, then the card lists every verdict, the attempts and the trajectory", () => {
    const m = proposalCardModel(proposal(), new Set(), OFF);
    expect(m.evidence.workflow).toBe("check");
    expect(m.evidence.verdicts).toEqual([{ label: 'answer: answer mentions "checks pass"', passed: true }]);
    expect(m.evidence.attempts).toBe("1 judged attempt");
    expect(m.evidence.trajectory).toContain("session s1-ab");
    expect(m.evidence.trajectory).toContain("sha256 cdcdcdcdcdcd");
    expect(m.evidence.model).toBe("gemma-4");
    expect(m.allPassed).toBe(true);
    expect(m.canAccept).toBe(true);
  });

  it("Given evidence without passing verdicts, then Accept is disabled (fails closed)", () => {
    const none = proposalCardModel(proposal({ evidence: { ...proposal().evidence, verdicts: [] } }), new Set(), OFF);
    expect(none.canAccept).toBe(false);
    const failed = proposal();
    failed.evidence.verdicts[0].passed = false;
    expect(proposalCardModel(failed, new Set(), OFF).canAccept).toBe(false);
  });

  it("Given a skill, then the card shows its name, description and body", () => {
    const m = proposalCardModel(proposal({ kind: "skill", content: { kind: "skill", skill_md: SKILL } }), new Set(), OFF);
    expect(m.kindLabel).toBe("Skill");
    expect(m.title).toBe("deploy-checklist");
    expect(m.subtitle).toBe("Checks a contract before deploy");
    expect(m.body).toBe("1. Run the tests.");
  });
});

describe("Feature: nothing is kept without the member (AC2)", () => {
  it("Given a decided proposal, then there is nothing to accept", () => {
    for (const state of [{ state: "rejected", by: "m", reason: "" }, { state: "persisted" }] as LearnProposal["state"][]) {
      const m = proposalCardModel(proposal({ state }), new Set(), OFF);
      expect(m.awaiting).toBe(false);
      expect(m.canAccept).toBe(false);
    }
  });

  it("Given a save that did not finish, then the member may accept again and sees why", () => {
    const m = proposalCardModel(proposal({ state: { state: "persist_failed", reason: "accepted before a restart; accept again" } }), new Set(), OFF);
    expect(m.awaiting).toBe(true);
    expect(m.canAccept).toBe(true);
    expect(m.stateLabel).toContain("accept again");
  });
});

describe("Feature: contradictions are surfaced, never merged (AC4)", () => {
  const contra = proposal({
    conflicts: [{ kind: "contradiction", existing_id: "proposal:lp-000000000000000000000001", detail: "a memory you accepted earlier says something different", blocking: false }],
  });

  it("Given an unacknowledged contradiction, then Accept is disabled and says why", () => {
    const m = proposalCardModel(contra, new Set(), OFF);
    expect(m.canAccept).toBe(false);
    expect(m.acceptBlocked).toMatch(/Acknowledge/);
    expect(m.conflicts[0].label).toBe("Contradicts a memory");
  });

  it("Given the member acknowledges it, then Accept is enabled and the acknowledgement is sent", () => {
    const acked = new Set(["proposal:lp-000000000000000000000001", "memory:not-a-conflict"]);
    const m = proposalCardModel(contra, acked, OFF);
    expect(m.canAccept).toBe(true);
    expect(acknowledgedFor(contra, acked)).toEqual(["proposal:lp-000000000000000000000001"]);
  });

  it("Given a blocking clash with a saved skill, then no acknowledgement can accept it", () => {
    const p = proposal({
      kind: "skill",
      content: { kind: "skill", skill_md: SKILL },
      conflicts: [{ kind: "same_name_skill", existing_id: "skill:user/deploy-checklist", detail: "never overwritten", blocking: true }],
    });
    const acked = new Set(["skill:user/deploy-checklist"]);
    expect(proposalCardModel(p, acked, OFF).canAccept).toBe(false);
    expect(acknowledgedFor(p, acked)).toEqual([]);
  });

  it("Given a contradicted memory in the ledger, then its row says both are kept and unresolved, and claims nothing it does not enforce", () => {
    const mem: LearnedMemory = {
      proposalId: "lp-000000000000000000000002",
      key: "deploy chain",
      value: "40204",
      belnap: "both",
      contradicts: ["lp-000000000000000000000001"],
      contentSha256: "ab",
      workflowId: "check",
      acceptedBy: "0xm",
      acceptedAtMs: 1,
      decisionSeq: 3,
      graph: { state: "stored", nodeId: "0a1b2c3d4e5f" },
    };
    const r = memoryRowModel(mem);
    expect(r.belnapLabel).toMatch(/unresolved/);
    expect(r.belnapLabel).toMatch(/both are kept/);
    // Recall leaves an unresolved memory out (core, fan-out 7), and the label says exactly that.
    expect(r.belnapLabel).toMatch(/Hermes does not recall it until you resolve it/);
    expect(r.belnapLabel).not.toMatch(/rel(y|ied) on/);
    expect(r.tone).toBe("warn");
    expect(memoryRowModel({ ...mem, belnap: "true" }).belnapLabel).toBeNull();
    expect(memoryRowModel({ ...mem, belnap: "true", graph: { state: "pending" } }).graphLabel).toMatch(/Waiting/);
    expect(memoryRowModel({ ...mem, graph: { state: "failed", detail: "no signing identity" } }).tone).toBe("danger");
  });
});

describe("Feature: publishing is HIC-1 and off until signed off (AC3)", () => {
  const saved = proposal({ kind: "skill", content: { kind: "skill", skill_md: SKILL }, state: { state: "persisted" } });

  it("Given a saved skill, then the card offers publishing with core's availability", () => {
    const m = proposalCardModel(saved, new Set(), OFF);
    expect(m.publish).toEqual(OFF);
    expect(proposalCardModel(saved, new Set(), null).publish?.enabled).toBe(false);
  });

  it("Given a memory or an undecided skill, then there is no publish control", () => {
    expect(proposalCardModel(proposal({ state: { state: "persisted" } }), new Set(), OFF).publish).toBeNull();
    expect(proposalCardModel({ ...saved, state: { state: "proposed" } }, new Set(), OFF).publish).toBeNull();
  });
});

// ---- resolving a contradiction (AC4: the member's way out of `both`) ----------------------------

function lm(id: string, value: string, over: Partial<LearnedMemory> = {}): LearnedMemory {
  return {
    proposalId: id,
    key: "deploy chain",
    value,
    belnap: "both",
    contradicts: [],
    contentSha256: "ab".repeat(32),
    workflowId: "check",
    acceptedBy: "0xm",
    acceptedAtMs: 1,
    decisionSeq: 1,
    graph: { state: "stored", nodeId: "0a1b2c3d4e5f" },
    ...over,
  };
}

describe("Feature: resolving a contradiction (AC4)", () => {
  const A = "lp-000000000000000000000001";
  const B = "lp-000000000000000000000002";
  const C = "lp-000000000000000000000003";

  it("Given two memories that contradict, then each offers to keep it and names what is set aside", () => {
    const mems = [lm(A, "1", { contradicts: [B] }), lm(B, "40204", { contradicts: [A] })];
    const choice = resolveChoice(mems, mems[1]);
    expect(choice).not.toBeNull();
    expect(choice?.keep).toBe(B);
    expect(choice?.retract).toEqual([A]);
    expect(choice?.confirm).toContain('Keep "40204"');
    expect(choice?.confirm).toContain('set aside "1"');
    expect(choice?.confirm).toMatch(/kept for the record/);
  });

  it("Given three memories, then keeping one sets aside every other one it still contradicts", () => {
    const mems = [lm(A, "1", { contradicts: [B, C] }), lm(B, "2", { contradicts: [A, C] }), lm(C, "3", { contradicts: [A, B] })];
    expect(resolveChoice(mems, mems[2])?.retract).toEqual([A, B]);
  });

  it("Given a settled, retracted or unknown-partner memory, then there is nothing to resolve", () => {
    expect(resolveChoice([lm(A, "1", { belnap: "true" })], lm(A, "1", { belnap: "true" }))).toBeNull();
    const gone = lm(A, "1", { belnap: "false", retractedFor: B });
    expect(resolveChoice([gone], gone)).toBeNull();
    // A partner this ledger does not hold and that is not a `memory:<id>` the app held cannot be
    // settled here.
    const orphan = lm(A, "1", { contradicts: ["mem-7"] });
    expect(resolveChoice([orphan], orphan)).toBeNull();
  });

  it("Given a retracted memory, then its row says it was set aside and what was kept", () => {
    const mems = [lm(A, "1", { belnap: "false", retractedFor: B, contradicts: [] }), lm(B, "40204", { belnap: "true" })];
    const r = memoryRowModel(mems[0], mems);
    expect(r.belnapLabel).toMatch(/Set aside/);
    expect(r.belnapLabel).toContain('"40204"');
    expect(r.tone).toBe("muted");
    const never = memoryRowModel(lm(A, "1", { belnap: "false", retractedFor: B, graph: { state: "retracted" } }), mems);
    expect(never.graphLabel).toMatch(/not stored/i);
  });
});

describe("Feature: resolving against a memory the app already held (fan-out 7)", () => {
  const A = "lp-000000000000000000000001";
  const B = "lp-000000000000000000000002";

  it("Given a learned memory that contradicts a memory the app held, then keeping it sets that one aside", () => {
    const m = lm(A, "40204", { contradicts: ["memory:mem-7"] });
    const c = resolveChoice([m], m);
    expect(c?.keep).toBe(A);
    expect(c?.retract).toEqual(["memory:mem-7"]);
    expect(c?.confirm).toContain("set aside the memory you already had");
  });

  it("Given learned and held partners, then keeping one sets every partner aside", () => {
    const mems = [lm(A, "1", { contradicts: [B, "memory:mem-7"] }), lm(B, "2", { contradicts: [A] })];
    expect(resolveChoice(mems, mems[0])?.retract).toEqual([B, "memory:mem-7"]);
  });

  it("Given a learned memory that contradicts a memory the app held, then it can be set aside for it", () => {
    const m = lm(A, "40204", { contradicts: ["memory:mem-7"] });
    expect(setAsideChoice(m)).toEqual({
      keep: "memory:mem-7",
      retract: [A],
      confirm: expect.stringContaining('Set aside "40204" for deploy chain and keep the memory you already had'),
    });
    expect(setAsideChoice(lm(A, "1", { contradicts: [B] }))).toBeNull();
    expect(setAsideChoice(lm(A, "1", { belnap: "true", contradicts: ["memory:mem-7"] }))).toBeNull();
  });

  it("Given a ref, then only a well-formed memory:<id> counts as a memory the app held", () => {
    expect(isKnownRef("memory:mem-7")).toBe(true);
    expect(isKnownRef("memory:")).toBe(false);
    expect(isKnownRef("memory:a b")).toBe(false);
    expect(isKnownRef("mem-7")).toBe(false);
    expect(isKnownRef(undefined)).toBe(false);
  });
});
